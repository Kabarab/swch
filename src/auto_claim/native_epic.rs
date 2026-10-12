//! Opt-in native Epic recovery. All pixels stay local; success is an API entitlement.
use super::{offers::Offer, providers::epic_owns_offer};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Deserialize)]
pub struct Word {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub line: u32,
}
#[derive(Deserialize)]
pub struct Snapshot {
    pub ok: bool,
    pub version: u32,
    pub hwnd: String,
    pub pid: u32,
    pub rect: Rect,
    pub width: f64,
    pub height: f64,
    pub image_hash: String,
    pub last_input_tick: u32,
    pub words: Vec<Word>,
}
#[derive(Clone, Serialize)]
pub struct Button {
    pub text: String,
    pub rect: Rect,
}
pub enum Decision {
    Wait,
    Owned,
    Click(Button),
    Attention(&'static str),
}

fn normalized(text: &str) -> String {
    super::catalog::canonical_game_title(text)
        .replace(['®', '™', '©'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn lines(s: &Snapshot) -> Vec<(String, Rect)> {
    let mut grouped: BTreeMap<u32, Vec<&Word>> = BTreeMap::new();
    for w in &s.words {
        grouped.entry(w.line).or_default().push(w);
    }
    grouped
        .into_values()
        .map(|mut words| {
            words.sort_by(|a, b| a.x.total_cmp(&b.x));
            (
                words
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                union(&words),
            )
        })
        .collect()
}
fn union(words: &[&Word]) -> Rect {
    let x = words.iter().map(|w| w.x).fold(f64::INFINITY, f64::min);
    let y = words.iter().map(|w| w.y).fold(f64::INFINITY, f64::min);
    Rect {
        x,
        y,
        width: words.iter().map(|w| w.x + w.width).fold(0.0, f64::max) - x,
        height: words.iter().map(|w| w.y + w.height).fold(0.0, f64::max) - y,
    }
}
fn buttons(s: &Snapshot, label: &str) -> Vec<Button> {
    let parts = label.split_whitespace().collect::<Vec<_>>();
    let mut grouped: BTreeMap<u32, Vec<&Word>> = BTreeMap::new();
    for w in &s.words {
        grouped.entry(w.line).or_default().push(w);
    }
    grouped
        .into_values()
        .flat_map(|mut words| {
            words.sort_by(|a, b| a.x.total_cmp(&b.x));
            words
                .windows(parts.len())
                .filter(|window| {
                    window
                        .iter()
                        .zip(&parts)
                        .all(|(w, p)| w.text.eq_ignore_ascii_case(p))
                })
                .map(|window| Button {
                    text: label.into(),
                    rect: union(window),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}
pub fn decide(s: &Snapshot, offer: &Offer, get_sent: bool) -> Decision {
    if !s.ok
        || s.version != 1
        || s.width < 700.0
        || s.height < 500.0
        || s.words.len() > 10000
        || !s.width.is_finite()
        || !s.height.is_finite()
        || s.image_hash.len() != 64
        || !s.image_hash.bytes().all(|b| b.is_ascii_hexdigit())
        || s.words.iter().any(|w| {
            w.text.len() > 1024
                || !w.x.is_finite()
                || !w.y.is_finite()
                || !w.width.is_finite()
                || !w.height.is_finite()
                || w.x < 0.0
                || w.y < 0.0
                || w.width <= 0.0
                || w.height <= 0.0
                || w.x + w.width > s.width + 1.0
                || w.y + w.height > s.height + 1.0
        })
    {
        return Decision::Attention("Не удалось безопасно прочитать окно Epic Launcher.");
    }
    if offer.launcher != "epic"
        || offer.namespace.starts_with("mobile:")
        || !offer.eligible(chrono::Utc::now().timestamp())
        || !offer.original_price.is_some_and(|p| p > 0)
        || offer.item_ids.is_empty()
        || !offer
            .ends_at
            .is_some_and(|end| end > chrono::Utc::now().timestamp())
    {
        return Decision::Attention("Epic не подтвердил действующую раздачу платной полной игры.");
    }
    let lines = lines(s);
    let text = lines
        .iter()
        .map(|(t, _)| t.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    if [
        "captcha",
        "verify you are human",
        "one more step",
        "security check",
        "checking your browser",
    ]
    .iter()
    .any(|p| text.contains(p))
    {
        return Decision::Attention(
            "Epic Launcher требует проверки безопасности. Автоматические нажатия остановлены.",
        );
    }
    if [
        "date of birth",
        "verify your age",
        "enter your age",
        "parental pin",
        "enter your pin",
        "enter password",
        "email address",
        "sign in to epic",
    ]
    .iter()
    .any(|p| text.contains(p))
    {
        return Decision::Attention("Epic Launcher требует входа или подтверждения пользователя. Автоматические нажатия остановлены.");
    }
    let title = normalized(&offer.title);
    let title_matches = lines.iter().filter(|(t, _)| normalized(t) == title).count();
    if title_matches != 1 {
        return Decision::Wait;
    }
    if lines
        .iter()
        .any(|(t, _)| matches!(normalized(t).as_str(), "in library" | "owned"))
    {
        return Decision::Owned;
    }
    let get = buttons(s, "Get");
    let add = buttons(s, "Add to library");
    let place = buttons(s, "Place order");
    if get.len() > 1 || add.len() > 1 || place.len() > 1 {
        return Decision::Attention(
            "В окне Epic несколько одинаковых кнопок. Получение остановлено.",
        );
    }
    if let Some(button) = add.first() {
        if get_sent && text.contains("this is free") && text.contains("add it to your library") {
            return Decision::Click(button.clone());
        }
        return Decision::Attention("Epic не подтвердил бесплатное добавление этой игры.");
    }
    if !place.is_empty() {
        return Decision::Attention("Оформление Place order в лаунчере пока требует пользователя: точный состав заказа не подтверждён.");
    }
    if let Some(button) = get.first() {
        if get_sent {
            return Decision::Wait;
        }
        let r = &button.rect;
        let free = s.words.iter().any(|w| {
            w.text.eq_ignore_ascii_case("free")
                && w.x > s.width * 0.55
                && w.y < r.y
                && r.y - w.y < 250.0
                && s.words.iter().any(|discount| {
                    discount.text.replace(['−', '–'], "-") == "-100%"
                        && discount.x > s.width * 0.55
                        && (discount.y - w.y).abs() < 25.0
                        && (discount.x - w.x).abs() < 350.0
                })
        });
        let base = lines.iter().any(|(t, b)| {
            normalized(t) == "base game" && b.x > s.width * 0.55 && b.y < r.y && r.y - b.y < 350.0
        });
        if free && base && r.x > s.width * 0.55 {
            return Decision::Click(button.clone());
        }
        return Decision::Attention(
            "В Epic Launcher не подтверждены бесплатная цена и полная версия выбранной игры.",
        );
    }
    Decision::Wait
}
pub fn product_uri(url: &str) -> Result<String, String> {
    if !super::platforms::product_url("epic", url) {
        return Err("Ссылка не ведёт на игру Epic.".into());
    }
    let u = reqwest::Url::parse(url).map_err(|_| "Некорректная ссылка Epic.")?;
    let segments = u
        .path_segments()
        .ok_or("Некорректная ссылка Epic.")?
        .collect::<Vec<_>>();
    let index = segments
        .iter()
        .position(|p| *p == "p")
        .ok_or("Нет адреса игры Epic.")?;
    if segments.len() != index + 2 {
        return Err("Неподдерживаемый адрес игры Epic.".into());
    }
    let slug = segments[index + 1];
    if slug.is_empty() || !slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err("Неподдерживаемый адрес игры Epic.".into());
    }
    Ok(format!("com.epicgames.launcher://store/product/{slug}"))
}

#[cfg(windows)]
async fn helper(request: serde_json::Value) -> Result<serde_json::Value, String> {
    use tokio::io::AsyncWriteExt;
    let system_root = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .ok_or("Не найден Windows PowerShell для распознавания Epic.")?;
    let powershell_root = system_root.join("System32/WindowsPowerShell/v1.0");
    let mut child = tokio::process::Command::new(powershell_root.join("powershell.exe"))
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            include_str!("native_epic_vision.ps1"),
        ])
        .env("PSModulePath", powershell_root.join("Modules"))
        .creation_flags(0x08000000)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "Не удалось запустить локальное распознавание Epic.")?;
    let mut input = child
        .stdin
        .take()
        .ok_or("Не удалось передать состояние окна Epic.")?;
    input
        .write_all(request.to_string().as_bytes())
        .await
        .map_err(|_| "Не удалось передать состояние окна Epic.")?;
    input
        .shutdown()
        .await
        .map_err(|_| "Не удалось завершить запрос распознавания.")?;
    drop(input);
    let output = tokio::time::timeout(Duration::from_secs(20), child.wait_with_output())
        .await
        .map_err(|_| "Распознавание Epic не ответило вовремя.")?
        .map_err(|_| "Распознавание Epic остановилось.")?;
    if output.stdout.len() > 2_000_000 {
        return Err("Состояние окна Epic слишком велико.".into());
    }
    let data: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Не удалось прочитать состояние Epic Launcher.")?;
    if data["ok"] != true {
        let code = data["error"].as_str().unwrap_or_default();
        return Err(match code {
            "snapshot_changed"=>"native_snapshot_changed",
            "user_input_changed"|"user_input_active"=>"Обнаружен ввод пользователя. Нативное получение остановлено; повтори, когда компьютер свободен.",
            "window_not_foreground"|"foreground_changed"|"window_occluded"=>"Окно Epic Launcher должно быть открыто в фокусе и не перекрыто. Повтори получение в свободном интерфейсе.",
            "english_ocr_unavailable"=>"Для нативного получения нужен установленный OCR Windows en-US.",
            "security_check"=>"Epic Launcher требует проверки безопасности. Автоматические нажатия остановлены.",
            _=>"Локальное распознавание не подтвердило безопасное нажатие в Epic Launcher.",
        }.into());
    }
    Ok(data)
}

#[cfg(windows)]
pub async fn claim(
    epic: &crate::launchers::epic::EpicAuthStore,
    account: &crate::storage::Account,
    offer: &Offer,
    timeout: u16,
) -> Result<(), String> {
    use crate::launchers::epic::EpicSessionManager;
    use serde_json::json;
    let _guard = crate::sys::process::LAUNCHER_SWITCH_LOCK
        .try_lock()
        .map_err(|_| "Другой лаунчер переключается. Нативное получение Epic отложено.")?;
    let availability = helper(json!({"mode":"availability"})).await?;
    if availability["available"] != true || availability["language"] != "en-US" {
        return Err("Для нативного получения нужен установленный OCR Windows en-US.".into());
    }
    let manager = EpicSessionManager::new();
    let games = epic
        .native_claim_game_paths()
        .await
        .map_err(|_| "Не удалось проверить запущенные игры Epic.")?;
    let mut binding = manager
        .native_claim_binding(&account.id, &games)
        .map_err(|e| e.to_string())?;
    if binding.is_none() {
        let token = epic
            .launcher_refresh_token(&account.id)
            .await
            .map_err(|_| "Не удалось подготовить сохранённый вход Epic Launcher.")?;
        manager
            .login_for_claim(
                &account.id,
                account.display_name.as_deref().unwrap_or(&account.username),
                &token,
                &games,
            )
            .await
            .map_err(|e| e.to_string())?;
        binding = manager
            .native_claim_binding(&account.id, &games)
            .map_err(|e| e.to_string())?;
    }
    let binding = binding.ok_or("Epic Launcher не подтвердил выбранный аккаунт.")?;
    let (pid, _) = binding;
    epic.switch_active_account(&account.id)
        .await
        .map_err(|_| "Не удалось отметить подтверждённый аккаунт Epic.")?;
    let uri = product_uri(&offer.url)?;
    let paths = manager.paths().map_err(|_| "Не найден Epic Launcher.")?;
    let exe = paths.app_bundle_path.ok_or("Не найден Epic Launcher.")?;
    tokio::process::Command::new(exe)
        .arg(uri)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| "Не удалось открыть раздачу в Epic Launcher.")?;
    let hwnd = main_window(pid)?;
    let started = tokio::time::Instant::now();
    let mut get_sent = false;
    let mut confirmed = false;
    let mut owned_seen = false;
    let mut baseline = None;
    let mut unstable = 0u8;
    let mut last_poll = tokio::time::Instant::now() - Duration::from_secs(3);
    while started.elapsed() < Duration::from_secs(u64::from(timeout)) {
        if !offer.eligible(chrono::Utc::now().timestamp()) {
            return Err("Раздача Epic закончилась.".into());
        }
        let bound = manager
            .native_claim_binding(&account.id, &games)
            .map_err(|e| e.to_string())?;
        if bound != Some(binding) {
            return Err("Аккаунт или процесс Epic изменился. Получение остановлено.".into());
        }
        if last_poll.elapsed() > Duration::from_secs(3) {
            last_poll = tokio::time::Instant::now();
            let token = epic
                .get_valid_access_token(&account.id)
                .await
                .map_err(|_| "Не удалось проверить доступ Epic.")?;
            let items = epic
                .client()
                .fetch_all_entitlements(&token, &account.id)
                .await
                .map_err(|_| "Не удалось подтвердить библиотеку Epic.")?;
            if epic_owns_offer(&items, &account.id, offer) {
                return Ok(());
            }
        }
        let data = helper(json!({"mode":"snapshot","pid":pid,"hwnd":hwnd.to_string()})).await?;
        let snapshot: Snapshot =
            serde_json::from_value(data).map_err(|_| "Не удалось прочитать распознавание Epic.")?;
        if snapshot.pid != pid || snapshot.hwnd != hwnd.to_string() {
            return Err("Окно Epic изменилось.".into());
        }
        if baseline.is_some_and(|tick| tick != snapshot.last_input_tick) {
            return Err("Обнаружен ввод пользователя. Нативное получение Epic остановлено.".into());
        }
        baseline = Some(snapshot.last_input_tick);
        match decide(&snapshot, offer, get_sent) {
            Decision::Attention(message) => return Err(message.into()),
            Decision::Owned => owned_seen = true,
            Decision::Click(button) if !confirmed => {
                if manager
                    .native_claim_binding(&account.id, &games)
                    .map_err(|e| e.to_string())?
                    != Some(binding)
                {
                    return Err(
                        "Аккаунт или процесс Epic изменился перед нажатием. Получение остановлено."
                            .into(),
                    );
                }
                let is_get = button.text == "Get";
                match helper(json!({"mode":"click","pid":pid,"hwnd":snapshot.hwnd,"rect":snapshot.rect,
                    "image_hash":snapshot.image_hash,"last_input_tick":snapshot.last_input_tick,"button":button})).await {
                    Ok(result)=>{get_sent|=is_get;confirmed=!is_get;baseline=result["last_input_tick"].as_u64().map(|v|v as u32);unstable=0;},
                    Err(e) if e=="native_snapshot_changed"&&unstable<3=>{unstable+=1;},
                    Err(e)=>return Err(if e=="native_snapshot_changed"{"Изображение Epic меняется. Нативное нажатие отменено.".into()}else{e}),
                }
            }
            _ => {}
        }
        tokio::time::sleep(Duration::from_millis(350)).await;
    }
    Err(if confirmed || owned_seen {
        "Epic не подтвердил лицензию в библиотеке. Проверь её перед повтором; успех не записан."
    } else if get_sent {
        "Get нажата, но бесплатное добавление не подтверждено. Заверши оформление в Epic Launcher и проверь библиотеку."
    } else {
        "Не удалось распознать выбранную игру и кнопку Get со скидкой 100%. Нажатий не было. Открой английский интерфейс магазина Epic и повтори."
    }.into())
}
#[cfg(not(windows))]
pub async fn claim(
    _epic: &crate::launchers::epic::EpicAuthStore,
    _account: &crate::storage::Account,
    _offer: &Offer,
    _timeout: u16,
) -> Result<(), String> {
    Err("Нативное получение Epic доступно только в Windows.".into())
}

#[cfg(windows)]
fn main_window(pid: u32) -> Result<isize, String> {
    #[repr(C)]
    struct Bounds {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(
            callback: unsafe extern "system" fn(isize, isize) -> i32,
            data: isize,
        ) -> i32;
        fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
        fn GetWindowTextW(hwnd: isize, text: *mut u16, len: i32) -> i32;
        fn IsWindowVisible(hwnd: isize) -> i32;
        fn GetClientRect(hwnd: isize, rect: *mut Bounds) -> i32;
    }
    struct Search {
        pid: u32,
        windows: Vec<isize>,
    }
    unsafe extern "system" fn visit(hwnd: isize, data: isize) -> i32 {
        let search = &mut *(data as *mut Search);
        let mut owner = 0;
        GetWindowThreadProcessId(hwnd, &mut owner);
        if owner == search.pid && IsWindowVisible(hwnd) != 0 {
            let mut text = [0u16; 128];
            let n = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
            let mut bounds = Bounds {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if String::from_utf16_lossy(&text[..n.max(0) as usize]) == "Epic Games Launcher"
                && GetClientRect(hwnd, &mut bounds) != 0
                && bounds.right - bounds.left >= 700
                && bounds.bottom - bounds.top >= 500
            {
                search.windows.push(hwnd);
            }
        }
        1
    }
    let mut search = Search {
        pid,
        windows: vec![],
    };
    unsafe {
        EnumWindows(visit, &mut search as *mut Search as isize);
    }
    if search.windows.len() != 1 {
        return Err("Нужно одно открытое окно магазина Epic Launcher.".into());
    }
    Ok(search.windows[0])
}

#[cfg(all(test, windows))]
mod helper_tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires local Windows OCR; synthetic data only, no real window actions"]
    async fn production_helper_transport_recognizes_synthetic_labels_and_rejects_invalid_binding() {
        let availability = helper(serde_json::json!({"mode":"availability"}))
            .await
            .unwrap();
        assert_eq!(availability["available"], true);
        assert_eq!(availability["language"], "en-US");
        let data = helper(serde_json::json!({"mode":"self_test"}))
            .await
            .unwrap();
        let text = data["words"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["text"].as_str().unwrap())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("Get"));
        assert!(text.contains("Add to library"));
        assert!(
            helper(serde_json::json!({"mode":"snapshot","hwnd":"0","pid":0}))
                .await
                .is_err()
        );
    }
}
