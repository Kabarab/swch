use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub published_at: Option<String>,
    pub html_url: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResponse {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_name: Option<String>,
    pub release_notes: Option<String>,
    pub published_at: Option<String>,
    pub html_url: Option<String>,
    pub asset_name: Option<String>,
    pub download_url: Option<String>,
    pub asset_size: Option<u64>,
    pub target_platform: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    Idle,
    Checking,
    Downloading,
    Extracting,
    Installing,
    Restarting,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStatusResponse {
    pub phase: UpdatePhase,
    pub progress_percent: u8,
    pub message: String,
    pub error: Option<String>,
    pub target_version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCheckParams {
    pub force: Option<bool>,
    pub repo: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateApplyRequest {
    pub download_url: Option<String>,
    pub asset_name: Option<String>,
    pub force: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct UpdateApplyResponse {
    pub status: String,
    pub message: String,
    pub relaunching: bool,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Compares two version strings (e.g. "1.1.1" vs "v1.2.0").
/// Returns true if `remote` is strictly newer than `local`.
pub fn is_newer_version(local: &str, remote: &str) -> bool {
    let local_clean = local.trim().trim_start_matches('v');
    let remote_clean = remote.trim().trim_start_matches('v');

    let local_parts = parse_semver(local_clean);
    let remote_parts = parse_semver(remote_clean);

    remote_parts > local_parts
}

pub fn parse_semver(v: &str) -> (u32, u32, u32) {
    let clean = v.trim().trim_start_matches(|c: char| !c.is_ascii_digit());
    let parts: Vec<&str> = clean.split(|c: char| c == '.' || c == '-' || c == '+').collect();
    let major = parts.get(0).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let minor = parts.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let patch = parts.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    (major, minor, patch)
}

/// Identifies the most appropriate release asset for the current OS and architecture.
pub fn find_best_asset<'a>(assets: &'a [ReleaseAsset], os: &str, arch: &str) -> Option<&'a ReleaseAsset> {
    let os_lower = os.to_lowercase();
    let arch_lower = arch.to_lowercase();

    // macOS
    if os_lower == "macos" || os_lower == "darwin" {
        let is_arm = arch_lower == "aarch64" || arch_lower == "arm64";
        if is_arm {
            if let Some(a) = assets.iter().find(|a| {
                let n = a.name.to_lowercase();
                (n.contains("macos") || n.contains("darwin") || n.contains("mac"))
                    && (n.contains("arm64") || n.contains("aarch64"))
            }) {
                return Some(a);
            }
            // Universal or x86_64 fallback (Apple Silicon can run x86_64 via Rosetta 2)
            if let Some(a) = assets.iter().find(|a| {
                let n = a.name.to_lowercase();
                (n.contains("macos") || n.contains("darwin") || n.contains("mac"))
                    && (n.contains("universal") || n.contains("x86_64") || n.contains("amd64") || n.contains("x64") || n.ends_with(".zip") || n.ends_with(".dmg"))
            }) {
                return Some(a);
            }
        } else {
            if let Some(a) = assets.iter().find(|a| {
                let n = a.name.to_lowercase();
                (n.contains("macos") || n.contains("darwin") || n.contains("mac"))
                    && (n.contains("x86_64") || n.contains("amd64") || n.contains("x64"))
            }) {
                return Some(a);
            }
            // Universal fallback for Intel (must NOT be arm64/aarch64)
            if let Some(a) = assets.iter().find(|a| {
                let n = a.name.to_lowercase();
                (n.contains("macos") || n.contains("darwin") || n.contains("mac"))
                    && n.contains("universal")
                    && !n.contains("arm64")
                    && !n.contains("aarch64")
            }) {
                return Some(a);
            }
        }
        return None;
    }

    // Windows
    if os_lower == "windows" {
        if let Some(a) = assets.iter().find(|a| {
            let n = a.name.to_lowercase();
            (n.contains("windows") || n.contains("win64") || n.contains("win") || n.contains("amd64") || n.contains("x86_64"))
                && (n.ends_with(".zip") || n.ends_with(".exe"))
        }) {
            return Some(a);
        }
        return None;
    }

    // Linux
    if os_lower == "linux" {
        if let Some(a) = assets.iter().find(|a| {
            let n = a.name.to_lowercase();
            n.contains("linux") && (n.ends_with(".tar.gz") || n.ends_with(".zip"))
        }) {
            return Some(a);
        }
        return None;
    }

    None
}

pub struct UpdateManager {
    state: Arc<Mutex<UpdateStatusResponse>>,
    client: reqwest::Client,
    pub default_repo: String,
}

impl Default for UpdateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdateManager {
    pub fn new() -> Self {
        let default_repo = std::env::var("SWCH_UPDATE_REPO")
            .unwrap_or_else(|_| "Kabarab/swch".to_string());

        let client = reqwest::Client::builder()
            .user_agent(format!("swch/{}", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .unwrap_or_default();

        Self {
            state: Arc::new(Mutex::new(UpdateStatusResponse {
                phase: UpdatePhase::Idle,
                progress_percent: 0,
                message: "Готов к проверке обновлений".to_string(),
                error: None,
                target_version: None,
            })),
            client,
            default_repo,
        }
    }

    pub async fn get_status(&self) -> UpdateStatusResponse {
        let st = self.state.lock().await;
        st.clone()
    }

    async fn fetch_json(&self, url: &str, token: Option<&str>) -> Result<String, String> {
        let mut req = self.client.get(url)
            .header("Accept", "application/vnd.github.v3+json");
        if let Some(t) = token {
            req = req.header("Authorization", format!("Bearer {}", t));
        }

        match req.send().await {
            Ok(resp) => {
                if resp.status() == reqwest::StatusCode::NOT_FOUND {
                    return Err("NOT_FOUND".to_string());
                }
                if resp.status() == reqwest::StatusCode::FORBIDDEN || resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    return Err("Превышен лимит запросов к GitHub API. Попробуйте позже.".to_string());
                }
                if !resp.status().is_success() {
                    return Err(format!("Ошибка GitHub API: HTTP {}", resp.status()));
                }
                resp.text().await.map_err(|e| format!("Ошибка чтения ответа: {}", e))
            }
            Err(err) => {
                // Fallback to system curl
                let mut cmd = std::process::Command::new("curl");
                cmd.arg("-s").arg("-f")
                    .arg("-H").arg("Accept: application/vnd.github.v3+json")
                    .arg("-H").arg(format!("User-Agent: swch/{}", env!("CARGO_PKG_VERSION")));
                if let Some(t) = token {
                    cmd.arg("-H").arg(format!("Authorization: Bearer {}", t));
                }
                cmd.arg(url);
                match cmd.output() {
                    Ok(out) if out.status.success() => {
                        String::from_utf8(out.stdout).map_err(|e| format!("UTF8 error: {}", e))
                    }
                    _ => Err(format!("Не удалось подключиться к GitHub API: {}", err)),
                }
            }
        }
    }

    pub async fn check_for_updates(
        &self,
        repo_override: Option<&str>,
        force: bool,
    ) -> Result<UpdateCheckResponse, String> {
        let repo = repo_override.unwrap_or(&self.default_repo);
        let url = format!("https://api.github.com/repos/{}/releases/latest", repo);

        let token = get_github_token();
        let release_json: String = match self.fetch_json(&url, token.as_deref()).await {
            Ok(j) => j,
            Err(e) if e == "NOT_FOUND" => {
                return Ok(UpdateCheckResponse {
                    current_version: env!("CARGO_PKG_VERSION").to_string(),
                    latest_version: env!("CARGO_PKG_VERSION").to_string(),
                    update_available: false,
                    release_name: None,
                    release_notes: Some("Релизов на GitHub пока не найдено.".to_string()),
                    published_at: None,
                    html_url: None,
                    asset_name: None,
                    download_url: None,
                    asset_size: None,
                    target_platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
                });
            }
            Err(e) => return Err(e),
        };

        let release: GitHubRelease = serde_json::from_str(&release_json)
            .map_err(|e| format!("Ошибка парсинга ответа GitHub: {}", e))?;

        let current_version = env!("CARGO_PKG_VERSION").to_string();
        let latest_version = release.tag_name.trim_start_matches('v').to_string();
        let is_newer = is_newer_version(&current_version, &release.tag_name);
        let update_available = is_newer || force;

        let best_asset = find_best_asset(&release.assets, std::env::consts::OS, std::env::consts::ARCH);

        Ok(UpdateCheckResponse {
            current_version,
            latest_version,
            update_available,
            release_name: release.name,
            release_notes: release.body,
            published_at: release.published_at,
            html_url: Some(release.html_url),
            asset_name: best_asset.map(|a| a.name.clone()),
            download_url: best_asset.map(|a| a.browser_download_url.clone()),
            asset_size: best_asset.map(|a| a.size),
            target_platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        })
    }

    pub async fn apply_update(&self, download_url: Option<String>, force: bool) -> Result<UpdateApplyResponse, String> {
        let mut st = self.state.lock().await;
        if st.phase == UpdatePhase::Downloading
            || st.phase == UpdatePhase::Extracting
            || st.phase == UpdatePhase::Installing
            || st.phase == UpdatePhase::Restarting
        {
            return Err("Обновление уже выполняется".to_string());
        }

        let target_url = match download_url {
            Some(u) => u,
            None => {
                drop(st);
                let check = self.check_for_updates(None, force).await?;
                let url = check.download_url.ok_or_else(|| "Не найден подходящий файл обновления для вашей ОС".to_string())?;
                st = self.state.lock().await;
                url
            }
        };

        st.phase = UpdatePhase::Downloading;
        st.progress_percent = 5;
        st.message = "Запуск скачивания обновления...".to_string();
        st.error = None;
        drop(st);

        let state_clone = self.state.clone();
        let client_clone = self.client.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::run_update_pipeline(state_clone.clone(), client_clone, target_url).await {
                error!("Update pipeline failed: {}", e);
                let mut st = state_clone.lock().await;
                st.phase = UpdatePhase::Error;
                st.error = Some(e.clone());
                st.message = format!("Ошибка обновления: {}", e);
            }
        });

        Ok(UpdateApplyResponse {
            status: "started".to_string(),
            message: "Скачивание и установка обновления начаты".to_string(),
            relaunching: false,
        })
    }

    async fn run_update_pipeline(
        state: Arc<Mutex<UpdateStatusResponse>>,
        client: reqwest::Client,
        url: String,
    ) -> Result<(), String> {
        info!("Starting update download from: {}", url);

        // 1. Download file
        {
            let mut st = state.lock().await;
            st.phase = UpdatePhase::Downloading;
            st.progress_percent = 15;
            st.message = "Скачивание обновления с GitHub...".to_string();
        }

        let temp_dir = std::env::temp_dir().join(format!("swch_update_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()));
        std::fs::create_dir_all(&temp_dir).map_err(|e| format!("Не удалось создать временную папку: {}", e))?;
        let archive_path = temp_dir.join("swch_package.zip");

        let mut downloaded = false;
        if let Ok(resp) = client.get(&url).send().await {
            if resp.status().is_success() {
                if let Ok(bytes) = resp.bytes().await {
                    if std::fs::write(&archive_path, &bytes).is_ok() {
                        downloaded = true;
                    }
                }
            }
        }

        if !downloaded {
            let curl_status = std::process::Command::new("curl")
                .arg("-sL")
                .arg("-o")
                .arg(&archive_path)
                .arg(&url)
                .status();
            if !curl_status.map(|s| s.success()).unwrap_or(false) || !archive_path.exists() {
                return Err("Не удалось загрузить архив обновления (ошибка сети)".to_string());
            }
        }

        {
            let mut st = state.lock().await;
            st.phase = UpdatePhase::Extracting;
            st.progress_percent = 55;
            st.message = "Распаковка архива обновления...".to_string();
        }

        let extract_dir = temp_dir.join("extracted");
        std::fs::create_dir_all(&extract_dir).map_err(|e| format!("Не удалось создать папку распаковки: {}", e))?;

        // Unpack archive using system unzip, tar, or powershell
        let mut extracted_ok = false;

        let unzip_status = std::process::Command::new("unzip")
            .arg("-q")
            .arg("-o")
            .arg(&archive_path)
            .arg("-d")
            .arg(&extract_dir)
            .status();

        if let Ok(s) = unzip_status {
            if s.success() {
                extracted_ok = true;
            }
        }

        if !extracted_ok {
            let tar_status = std::process::Command::new("tar")
                .arg("-xf")
                .arg(&archive_path)
                .arg("-C")
                .arg(&extract_dir)
                .status();
            if let Ok(s) = tar_status {
                if s.success() {
                    extracted_ok = true;
                }
            }
        }

        #[cfg(windows)]
        if !extracted_ok {
            let ps_status = std::process::Command::new("powershell")
                .arg("-NoProfile")
                .arg("-Command")
                .arg(format!(
                    "Expand-Archive -Force -Path '{}' -DestinationPath '{}'",
                    archive_path.display(),
                    extract_dir.display()
                ))
                .status();
            if let Ok(s) = ps_status {
                if s.success() {
                    extracted_ok = true;
                }
            }
        }

        if !extracted_ok {
            return Err("Не удалось распаковать архив обновления".to_string());
        }

        // 2. Install update
        {
            let mut st = state.lock().await;
            st.phase = UpdatePhase::Installing;
            st.progress_percent = 85;
            st.message = "Установка новой версии swch...".to_string();
        }

        let current_exe = std::env::current_exe().map_err(|e| format!("Не удалось определить путь к текущему исполняемому файлу: {}", e))?;
        info!("Current executable: {}", current_exe.display());

        // Check if running inside macOS .app bundle
        let running_bundle = find_enclosing_app_bundle(&current_exe);
        let extracted_bundle = find_app_in_dir(&extract_dir);

        let relaunch_cmd: (PathBuf, Vec<String>) = match (running_bundle, extracted_bundle) {
            (Some(running_app), Some(extracted_app)) => {
                info!("Updating macOS .app bundle: {} with {}", running_app.display(), extracted_app.display());
                // Validate destination write permissions
                if is_path_readonly(&running_app) {
                    return Err("Невозможно обновить: приложение запущено из образа только для чтения (например, DMG). Скопируйте swch.app в Программы!".to_string());
                }

                let backup_app = running_app.with_extension("app.old");
                if backup_app.exists() {
                    let _ = std::fs::remove_dir_all(&backup_app);
                }

                std::fs::rename(&running_app, &backup_app)
                    .map_err(|e| format!("Не удалось переименовать текущее приложение: {}", e))?;

                if let Err(e) = move_or_copy_dir(&extracted_app, &running_app) {
                    // Rollback
                    let _ = move_or_copy_dir(&backup_app, &running_app);
                    return Err(format!("Не удалось установить новое приложение: {}", e));
                }

                // Ensure execution permissions on macOS binaries
                let macos_dir = running_app.join("Contents/MacOS");
                if macos_dir.exists() {
                    let _ = std::process::Command::new("chmod")
                        .arg("-R")
                        .arg("+x")
                        .arg(&macos_dir)
                        .status();
                }

                // Terminate any running GUI instances of swch associated with this bundle
                // to prevent duplicate windows when the new version opens
                let mut sys = sysinfo::System::new();
                sys.refresh_processes();
                let my_pid = std::process::id();
                for (pid, proc) in sys.processes() {
                    if pid.as_u32() != my_pid && proc.name().eq_ignore_ascii_case("swch") {
                        if let Some(exe) = proc.exe() {
                            if exe.starts_with(&running_app) || exe.to_string_lossy().contains("swch.app") {
                                info!("Terminating existing GUI process {} before relaunch", pid);
                                let _ = proc.kill_with(sysinfo::Signal::Term);
                            }
                        }
                    }
                }

                (
                    PathBuf::from("/bin/sh"),
                    vec![
                        "-c".to_string(),
                        format!("sleep 1 && /usr/bin/open {:?}", running_app),
                    ],
                )
            }
            _ => {
                // Standalone binary update
                let current_stem = current_exe.file_stem().and_then(|s| s.to_str()).unwrap_or("swch");
                let bin_target = if current_stem == "swch-server" { "swch-server" } else { "swch" };
                let new_bin = find_binary_in_dir(&extract_dir, bin_target)
                    .or_else(|| find_binary_in_dir(&extract_dir, "swch"))
                    .ok_or_else(|| format!("Исполняемый файл {} не найден в распакованном архиве", bin_target))?;

                info!("Updating standalone binary: {} with {}", current_exe.display(), new_bin.display());

                replace_executable(&current_exe, &new_bin)?;

                // Also update index.html and ui/ if present in package
                let exe_dir = current_exe.parent().unwrap_or_else(|| Path::new("."));
                let new_index = extract_dir.join("index.html");
                if new_index.exists() {
                    let _ = std::fs::copy(&new_index, exe_dir.join("index.html"));
                }
                let new_ui = extract_dir.join("ui");
                if new_ui.is_dir() {
                    let _ = copy_dir_all(&new_ui, &exe_dir.join("ui"));
                }

                #[cfg(windows)]
                {
                    (
                        PathBuf::from("cmd"),
                        vec![
                            "/C".to_string(),
                            "start".to_string(),
                            "".to_string(),
                            current_exe.to_string_lossy().to_string(),
                        ],
                    )
                }
                #[cfg(not(windows))]
                {
                    (
                        PathBuf::from("/bin/sh"),
                        vec![
                            "-c".to_string(),
                            format!("sleep 1 && {:?}", current_exe),
                        ],
                    )
                }
            }
        };

        // Clean up temp directory
        let _ = std::fs::remove_dir_all(&temp_dir);

        // 3. Restart
        {
            let mut st = state.lock().await;
            st.phase = UpdatePhase::Restarting;
            st.progress_percent = 100;
            st.message = "Обновление завершено! Перезапуск приложения...".to_string();
        }

        info!("Update completed successfully. Scheduling relaunch in 800ms...");
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            let (bin, args) = relaunch_cmd;
            let mut cmd = std::process::Command::new(&bin);
            cmd.args(&args);
            let _ = cmd.spawn();
            std::process::exit(0);
        });

        Ok(())
    }
}

fn get_github_token() -> Option<String> {
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        let t = token.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    if let Ok(output) = std::process::Command::new("gh").arg("auth").arg("token").output() {
        if output.status.success() {
            let t = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

pub fn find_enclosing_app_bundle(path: &Path) -> Option<PathBuf> {
    let mut cur = path;
    while let Some(parent) = cur.parent() {
        if parent.extension().and_then(|e| e.to_str()) == Some("app") {
            return Some(parent.to_path_buf());
        }
        cur = parent;
    }
    None
}

pub fn find_app_in_dir(dir: &Path) -> Option<PathBuf> {
    for entry in walkdir::WalkDir::new(dir).max_depth(3).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_dir() && entry.path().extension().and_then(|e| e.to_str()) == Some("app") {
            return Some(entry.path().to_path_buf());
        }
    }
    None
}

pub fn find_binary_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { format!("{}.exe", name) } else { name.to_string() };
    for entry in walkdir::WalkDir::new(dir).max_depth(4).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            if let Some(file_name) = entry.path().file_name().and_then(|f| f.to_str()) {
                if file_name == exe_name {
                    return Some(entry.path().to_path_buf());
                }
            }
        }
    }
    None
}

fn is_path_readonly(path: &Path) -> bool {
    let test_file = path.join(".swch_write_test");
    match std::fs::write(&test_file, b"test") {
        Ok(_) => {
            let _ = std::fs::remove_file(&test_file);
            false
        }
        Err(_) => true,
    }
}

pub fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

pub fn move_or_copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    // Fallback: cross-device or permission failure with rename
    #[cfg(unix)]
    {
        // On macOS / Unix, use cp -R to preserve bundle structure, symlinks, and permissions
        let status = std::process::Command::new("cp")
            .arg("-R")
            .arg(src)
            .arg(dst)
            .status();
        if let Ok(s) = status {
            if s.success() {
                let _ = std::fs::remove_dir_all(src);
                return Ok(());
            }
        }
    }
    // Pure Rust recursive copy fallback
    copy_dir_all(src, dst)?;
    let _ = std::fs::remove_dir_all(src);
    Ok(())
}

pub fn replace_executable(current_exe: &Path, new_bin: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let old_exe = current_exe.with_extension("exe.old");
        if old_exe.exists() {
            let _ = std::fs::remove_file(&old_exe);
        }
        std::fs::rename(current_exe, &old_exe)
            .map_err(|e| format!("Не удалось переименовать текущий файл (Windows): {}", e))?;

        if let Err(e) = std::fs::copy(new_bin, current_exe) {
            let _ = std::fs::rename(&old_exe, current_exe);
            return Err(format!("Не удалось скопировать новый файл: {}", e));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let temp_new_bin = current_exe.with_extension("new_exe");
        std::fs::copy(new_bin, &temp_new_bin)
            .map_err(|e| format!("Не удалось скопировать новый файл: {}", e))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&temp_new_bin, std::fs::Permissions::from_mode(0o755));
        }

        std::fs::rename(&temp_new_bin, current_exe)
            .map_err(|e| format!("Не удалось заменить исполняемый файл: {}", e))?;
        Ok(())
    }
}

// ---------------------------------------------------------
// Axum HTTP Handlers
// ---------------------------------------------------------

pub async fn check_update(
    State(update_mgr): State<Arc<UpdateManager>>,
    Query(params): Query<UpdateCheckParams>,
) -> Result<Json<UpdateCheckResponse>, (StatusCode, Json<ErrorResponse>)> {
    match update_mgr.check_for_updates(params.repo.as_deref(), params.force.unwrap_or(false)).await {
        Ok(res) => Ok(Json(res)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )),
    }
}

pub async fn apply_update(
    State(update_mgr): State<Arc<UpdateManager>>,
    Json(payload): Json<UpdateApplyRequest>,
) -> Result<Json<UpdateApplyResponse>, (StatusCode, Json<ErrorResponse>)> {
    match update_mgr.apply_update(payload.download_url, payload.force.unwrap_or(false)).await {
        Ok(res) => Ok(Json(res)),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: e }),
        )),
    }
}

pub async fn update_status(
    State(update_mgr): State<Arc<UpdateManager>>,
) -> Result<Json<UpdateStatusResponse>, (StatusCode, Json<ErrorResponse>)> {
    Ok(Json(update_mgr.get_status().await))
}
