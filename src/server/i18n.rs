//! Persist a validated language choice for browser windows and native startup.
use axum::{
    extract::{DefaultBodyLimit, Path},
    http::StatusCode,
    routing::get,
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf};
#[derive(Deserialize, Serialize)]
pub struct Selection {
    pub locale: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preference: Option<String>,
    pub revision: String,
    pub machine: bool,
    pub messages: BTreeMap<String, String>,
}

/// Read display-language preferences rather than numeric/date region settings.
pub fn system_languages() -> Vec<String> {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetUserPreferredUILanguages(
                flags: u32,
                count: *mut u32,
                buffer: *mut u16,
                size: *mut u32,
            ) -> i32;
        }
        let (mut count, mut size) = (0, 0);
        unsafe {
            GetUserPreferredUILanguages(8, &mut count, std::ptr::null_mut(), &mut size);
        }
        if (2..=32768).contains(&size) {
            let mut buffer = vec![0u16; size as usize];
            if unsafe { GetUserPreferredUILanguages(8, &mut count, buffer.as_mut_ptr(), &mut size) }
                != 0
            {
                let languages: Vec<_> = buffer
                    .split(|c| *c == 0)
                    .filter(|s| !s.is_empty())
                    .map(String::from_utf16_lossy)
                    .filter(|s| valid_locale(s))
                    .collect();
                if !languages.is_empty() {
                    return languages;
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        // The native launcher passes Foundation's ordered preferred languages.
        if let Ok(languages) = std::env::var("SWCH_SYSTEM_LANGUAGES") {
            let languages: Vec<_> = languages
                .split(',')
                .filter(|s| valid_locale(s))
                .map(str::to_owned)
                .collect();
            if !languages.is_empty() {
                return languages;
            }
        }
        if let Ok(output) = std::process::Command::new("/usr/bin/defaults")
            .args(["read", "-g", "AppleLanguages"])
            .output()
        {
            if output.status.success() {
                let languages: Vec<_> = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(|line| {
                        line.trim()
                            .trim_matches(|c: char| matches!(c, '"' | ',' | '(' | ')'))
                    })
                    .filter(|s| valid_locale(s))
                    .map(str::to_owned)
                    .collect();
                if !languages.is_empty() {
                    return languages;
                }
            }
        }
    }
    for variable in ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"] {
        if let Ok(value) = std::env::var(variable) {
            let languages: Vec<_> = value
                .split(':')
                .map(|s| s.split(['.', '@']).next().unwrap_or("").replace('_', "-"))
                .filter(|s| valid_locale(s))
                .collect();
            if !languages.is_empty() {
                return languages;
            }
        }
    }
    vec!["en".into()]
}
pub fn directory() -> PathBuf {
    std::env::var_os("SWCH_I18N_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("swch")
                .join("i18n")
        })
}
#[derive(Clone)]
struct SourceCatalog(PathBuf);

pub fn router() -> Router<super::AppState> {
    let index_path = super::resolve_index_path();
    let source_path = index_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("ui/lang/source.json");
    Router::new()
        .route("/api/i18n/selection", get(selection).post(save))
        .route("/api/i18n/packs/:locale", get(pack))
        .layer(Extension(SourceCatalog(source_path)))
        .layer(DefaultBodyLimit::max(4_000_000))
        .layer(axum::middleware::from_fn(super::mods::local_origin))
}
fn snapshot() -> Option<PathBuf> {
    let active = std::fs::read_to_string(directory().join("active.txt")).ok()?;
    if active.len() != 64 || !active.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(directory().join("versions").join(active))
}
async fn selection() -> Json<Value> {
    let value = snapshot()
        .and_then(|dir| std::fs::read(dir.join("selection.json")).ok())
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .filter(|value| value["locale"].as_str().is_some_and(valid_locale));
    let saved = value.is_some();
    let mut value = value.unwrap_or_else(|| json!({"locale":"en","preference":"system"}));
    if value["preference"].as_str().is_none() {
        value["preference"] = value["locale"].clone();
    }
    value["saved"] = json!(saved);
    value["systemLanguages"] = json!(system_languages());
    Json(value)
}
async fn pack(Path(locale): Path<String>) -> Result<Json<Value>, StatusCode> {
    if !valid_locale(&locale) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let bytes = std::fs::read(directory().join("packs").join(locale + ".json"))
        .map_err(|_| StatusCode::NOT_FOUND)?;
    serde_json::from_slice(&bytes)
        .map(Json)
        .map_err(|_| StatusCode::NOT_FOUND)
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn valid_locale(locale: &str) -> bool {
    let parts: Vec<_> = locale.split('-').collect();
    parts.len() <= 3
        && (2..=3).contains(&parts[0].len())
        && parts[0].bytes().all(|b| b.is_ascii_alphabetic())
        && parts[1..]
            .iter()
            .all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}
fn slots(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    for suffix in text.split('{').skip(1) {
        if let Some((candidate, _)) = suffix.split_once('}') {
            if !candidate.is_empty() && candidate.bytes().all(|b| b.is_ascii_digit()) {
                result.push(candidate.to_owned());
            }
        }
    }
    result.sort();
    result
}
pub fn validate(selection: &Selection, source: &Value) -> bool {
    let Some(messages) = source["messages"].as_object() else {
        return false;
    };
    valid_locale(&selection.locale)
        && selection
            .preference
            .as_deref()
            .is_none_or(|p| p == "system" || p == selection.locale)
        && source["revision"].as_str() == Some(selection.revision.as_str())
        && selection.messages.len() == messages.len()
        && messages.keys().all(|key| {
            selection.messages.get(key).is_some_and(|text| {
                !text.trim().is_empty() && text.len() <= 64_000 && slots(text) == slots(key)
            })
        })
}
pub fn startup_html(selection: &Selection, error: bool, windows: bool) -> String {
    let text = |key: &str| {
        escape(
            selection
                .messages
                .get(key)
                .map(String::as_str)
                .unwrap_or_else(|| match key {
                    "Не удалось запустить SWCH" => "Unable to start SWCH",
                    "Запуск SWCH..." => "Starting SWCH...",
                    "Серверный движок не ответил вовремя. Проверьте запуск приложения или повторите попытку." => "The backend did not respond in time. Check that the application is running or try again.",
                    "Инициализация игрового пространства" => "Initializing your game library",
                    "Повторить попытку" => "Try again",
                    _ => key,
                }),
        )
    };
    let title = text(if error {
        "Не удалось запустить SWCH"
    } else {
        "Запуск SWCH..."
    });
    let description = text(if error {
        "Серверный движок не ответил вовремя. Проверьте запуск приложения или повторите попытку."
    } else {
        "Инициализация игрового пространства"
    });
    let retry = if error {
        format!(
            "<button onclick=\"{}\">{}</button>",
            if windows {
                "window.chrome.webview.postMessage('retry')"
            } else {
                "location.href='http://127.0.0.1:4242/'"
            },
            text("Повторить попытку")
        )
    } else {
        String::new()
    };
    let rtl = [
        "ar", "fa", "iw", "he", "ur", "ps", "sd", "ug", "dv", "yi", "ckb", "bal",
    ]
    .iter()
    .any(|code| selection.locale.split('-').next() == Some(*code))
        || selection.locale.contains("-Arab")
        || selection.locale.contains("-Nkoo");
    format!("<!doctype html><html lang=\"{}\" dir=\"{}\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>SWCH</title><style>body{{margin:0;background:#111214;color:#f2f2f4;font:16px/1.5 system-ui,sans-serif;display:grid;min-height:100vh;place-items:center}}main{{text-align:center;max-width:440px;padding:32px;overflow-wrap:anywhere}}p{{color:#adb0bb}}button{{background:#ff642d;border:0;padding:12px 22px;border-radius:8px;font:inherit;white-space:normal}}</style><main><h2>{}</h2><p>{}</p>{}</main></html>",escape(&selection.locale),if rtl {"rtl"} else {"ltr"},title,description,retry)
}
async fn save(
    Extension(SourceCatalog(source_path)): Extension<SourceCatalog>,
    Json(choice): Json<Selection>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // The UI files can be updated independently of the server executable.
    // Validate against the same catalog served to the browser, including updates
    // made after startup, rather than a snapshot embedded at compile time.
    let source: Value = std::fs::read(&source_path)
        .map_err(|error| error.to_string())
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string()))
        .map_err(|error| {
            tracing::warn!(path = %source_path.display(), %error, "Could not read UI language catalog");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"language-catalog-unavailable"})),
            )
        })?;
    if !validate(&choice, &source) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid-language-pack"})),
        ));
    }
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let root = directory();
        let id = format!("{:x}", Sha256::digest(serde_json::to_vec(&choice)?));
        let dir = root.join("versions").join(&id);
        std::fs::create_dir_all(&dir)?;
        for (name, error, windows) in [
            ("loading-windows.html", false, true),
            ("error-windows.html", true, true),
            ("loading-macos.html", false, false),
            ("error-macos.html", true, false),
        ] {
            crate::storage::atomic::atomic_write(
                dir.join(name),
                startup_html(&choice, error, windows),
            )?;
        }
        crate::storage::atomic::atomic_write(
            dir.join("selection.json"),
            serde_json::to_vec(
                &json!({"locale":choice.locale,"preference":choice.preference.as_deref().unwrap_or(&choice.locale),"revision":choice.revision}),
            )?,
        )?;
        std::fs::create_dir_all(root.join("packs"))?;
        crate::storage::atomic::atomic_write(
            root.join("packs").join(choice.locale.clone() + ".json"),
            serde_json::to_vec(
                &json!({"schema":1,"locale":choice.locale,"revision":choice.revision,"machine":choice.machine,"messages":choice.messages}),
            )?,
        )?;
        // A single atomic pointer commits all native templates and the selection together.
        crate::storage::atomic::atomic_write(root.join("active.txt"), id)?;
        Ok(())
    })();
    result.map(|_| Json(json!({"status":"ok"}))).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"language-save-failed"})),
        )
    })
}
