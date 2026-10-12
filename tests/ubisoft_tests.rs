use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::fs;
use swch::{
    launchers::ubisoft::{
        games_from_registry, launch_uri,
        session::{account_uuid, user_from_log, user_from_process_log, SessionManager},
    },
    modules::{LauncherDetector, ModuleId},
    storage::Database,
};
use tempfile::{tempdir, TempDir};
use tower::ServiceExt;

const A: &str = "12345678-1111-2222-3333-123456789abc";
const B: &str = "87654321-1111-2222-3333-123456789abc";

fn fixture() -> (TempDir, SessionManager) {
    let dir = tempdir().unwrap();
    let manager = SessionManager {
        data_dir: dir.path().join("data"),
        launcher_dir: Some(dir.path().join("client")),
        sessions_dir: dir.path().join("sessions"),
    };
    fs::create_dir_all(&manager.data_dir).unwrap();
    fs::create_dir_all(manager.launcher_dir.as_ref().unwrap().join("logs")).unwrap();
    (dir, manager)
}
fn login(manager: &SessionManager, id: &str, token: &str) {
    fs::write(manager.data_dir.join("user.dat"), token).unwrap();
    fs::write(
        manager
            .launcher_dir
            .as_ref()
            .unwrap()
            .join("logs/launcher_log.txt"),
        format!("User: {id}\n"),
    )
    .unwrap();
}

#[test]
fn module_identifiers_detection_and_serialization() {
    for alias in ["ubisoft", "UBISOFT", "uplay", "ubi", "ubisoft_connect"] {
        assert_eq!(alias.parse::<ModuleId>().unwrap(), ModuleId::Ubisoft);
    }
    assert_eq!(
        serde_json::to_string(&ModuleId::Ubisoft).unwrap(),
        "\"ubisoft\""
    );
    let detector = LauncherDetector::with_mock_launchers(vec!["ubisoft".into()]);
    assert!(detector.is_installed(ModuleId::Ubisoft));
    assert!(!detector.is_installed(ModuleId::Steam));
}

#[test]
fn registry_uses_real_install_ids_deduplicates_and_skips_stale_entries() {
    let dir = tempdir().unwrap();
    let game = dir.path().join("Rainbow Six Siege");
    fs::create_dir(&game).unwrap();
    let text = format!("HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs\\635\n    InstallDir    REG_SZ    {}\n    DisplayName    REG_SZ    Tom Clancy's Rainbow Six Siege\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs\\635\n    InstallDir    REG_SZ    {}\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs\\999\n    InstallDir    REG_SZ    {}\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Ubisoft\\Launcher\\Installs\\635\\nested\n    InstallDir    REG_SZ    {}", game.display(), game.display(), dir.path().join("missing").display(), game.display());
    let games = games_from_registry(&text);
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].app_id, "635");
    assert_eq!(games[0].name, "Tom Clancy's Rainbow Six Siege");
    assert_eq!(games[0].launch_uri.as_deref(), Some("uplay://launch/635/0"));
}

#[test]
fn launch_and_account_ids_cannot_inject_paths_or_commands() {
    for id in [
        "",
        "../635",
        "635/1",
        "635 & calc",
        "uplay://launch/635/0",
        "1234567890123",
    ] {
        assert!(launch_uri(id).is_err());
    }
    assert_eq!(launch_uri("635").unwrap(), "uplay://launch/635/0");
    for id in [
        "",
        "../session",
        "..\\session",
        "12345678-1111-2222-3333-123456789abg",
    ] {
        assert!(account_uuid(id).is_err());
    }
    assert_eq!(
        account_uuid(&format!("ubisoft_{}", A.to_uppercase())).unwrap(),
        A
    );
}

#[test]
fn log_identity_uses_last_valid_authenticated_user() {
    assert_eq!(
        user_from_log(&format!("User: {A}\nUser: {B}\nUser: unavailable")),
        Some(B.into())
    );
    assert_eq!(user_from_log("no account"), None);
}

#[test]
fn restarted_login_window_does_not_reuse_the_previous_user() {
    let start = "-- Starting Ubisoft Game Launcher --";
    assert_eq!(
        user_from_log(&format!("{start}\nUser: {A}\n{start}\nStartView.cpp")),
        None
    );
    assert_eq!(
        user_from_log(&format!("{start}\nUser: {A}\n{start}\nUser: {B}")),
        Some(B.into())
    );
}

#[test]
fn activity_uses_the_running_client_pid_instead_of_previous_launches() {
    let log =
        format!("[ 11] User: {A}\n[ 22] -- Starting Ubisoft Game Launcher --\n[ 11] User: {A}");
    assert_eq!(user_from_process_log(&log, &[22]), None);
    assert_eq!(
        user_from_process_log(&format!("{log}\n[ 22] User: {B}"), &[22]),
        Some(B.into())
    );
    assert_eq!(user_from_process_log(&log, &[33]), None);
}

#[test]
fn preparing_another_login_preserves_sessions_and_preferences() {
    let (_dir, manager) = fixture();
    login(&manager, A, "remembered A");
    fs::write(manager.data_dir.join("settings.yaml"), "language:\r\n  code: ru\r\nmasters:\r\n  username: private-login\r\n  password: opaque-remembered-value\r\nuser:\r\n  offline: false\r\n").unwrap();
    manager.save_current(Some("First")).unwrap();
    login(&manager, B, "remembered B");
    manager.save_current(Some("Second")).unwrap();
    let before = fs::read(manager.sessions_dir.join(format!("{B}.json"))).unwrap();
    manager.prepare_login().unwrap();
    assert!(manager.current_id().is_none());
    assert!(manager.authenticated_id().is_none());
    assert!(manager.accounts().unwrap().iter().all(|a| !a.is_active));
    assert!(!manager.data_dir.join("user.dat").exists());
    assert_eq!(
        fs::read(manager.sessions_dir.join(format!("{B}.json"))).unwrap(),
        before
    );
    let settings = fs::read_to_string(manager.data_dir.join("settings.yaml")).unwrap();
    assert!(settings.contains("  code: ru\r\n"));
    assert!(settings.contains("  offline: false\r\n"));
    assert!(settings.contains("  username: \"\"\r\n"));
    assert!(!settings.contains("private-login"));
    assert!(!settings.contains("opaque-remembered-value"));
    manager.restore(A).unwrap();
    assert_eq!(
        fs::read(manager.data_dir.join("user.dat")).unwrap(),
        b"remembered A"
    );
}

#[test]
fn failed_login_cannot_overwrite_a_saved_session() {
    let (_dir, manager) = fixture();
    login(&manager, A, "remembered token");
    let saved = manager.save_current(Some("First")).unwrap();
    let path = manager.sessions_dir.join(format!("{A}.json"));
    let before = fs::read(&path).unwrap();
    fs::write(manager.data_dir.join("user.dat"), "logged out data").unwrap();
    fs::write(
        manager
            .launcher_dir
            .as_ref()
            .unwrap()
            .join("logs/launcher_log.txt"),
        format!("User: {A}\n-- Starting Ubisoft Game Launcher --\nStartView.cpp"),
    )
    .unwrap();
    assert!(manager.authenticated_id().is_none());
    assert!(manager.save_current(None).is_err());
    assert_eq!(fs::read(path).unwrap(), before);
    manager.restore(&saved.id).unwrap();
    // Restored local files can identify an account but do not prove a new login.
    assert_eq!(manager.current_id().as_deref(), Some(saved.id.as_str()));
    assert!(manager.save_current(None).is_err());
}

#[cfg(windows)]
#[tokio::test]
async fn closing_client_stops_main_and_helpers_only_in_its_installation() {
    use std::{os::windows::process::CommandExt, process::Command};
    let (_dir, manager) = fixture();
    let other = tempdir().unwrap();
    let command =
        std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap()).join("System32/cmd.exe");
    let spawn = |path: &std::path::Path| {
        fs::copy(&command, path).unwrap();
        Command::new(path)
            .args(["/C", "ping -n 30 127.0.0.1 >nul"])
            .creation_flags(0x08000000)
            .spawn()
            .unwrap()
    };
    let mut main = spawn(&manager.launcher_dir.as_ref().unwrap().join("upc.exe"));
    let mut helper = spawn(
        &manager
            .launcher_dir
            .as_ref()
            .unwrap()
            .join("UplayWebCore.exe"),
    );
    let mut unrelated = spawn(&other.path().join("upc.exe"));
    let result = manager.close_client().await;
    let main_stopped = main.try_wait().unwrap().is_some();
    let helper_stopped = helper.try_wait().unwrap().is_some();
    let unrelated_alive = unrelated.try_wait().unwrap().is_none();
    // Reap every test process even if an assertion fails.
    for child in [&mut main, &mut helper, &mut unrelated] {
        let _ = swch::sys::process::send_sigkill(child.id());
        let _ = child.kill();
        let _ = child.wait();
    }
    result.unwrap();
    assert!(main_stopped && helper_stopped);
    assert!(unrelated_alive);
}

#[test]
fn sessions_switch_and_survive_restart_without_leaking_credentials() {
    let (_dir, manager) = fixture();
    login(&manager, A, "token A");
    fs::write(manager.data_dir.join("settings.yaml"), "settings A").unwrap();
    let first = manager.save_current(Some("Основной")).unwrap();
    login(&manager, B, "token B");
    fs::write(
        manager.data_dir.join("ConnectSecureStorage.dat"),
        "secure B",
    )
    .unwrap();
    manager.save_current(Some("Второй")).unwrap();
    manager.restore(&first.id).unwrap();
    assert_eq!(
        fs::read(manager.data_dir.join("user.dat")).unwrap(),
        b"token A"
    );
    assert!(!manager.data_dir.join("ConnectSecureStorage.dat").exists());
    assert_eq!(manager.clone().current_id(), Some(first.id));
    let accounts = manager.clone().accounts().unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts.iter().filter(|a| a.is_active).count(), 1);
    let output = serde_json::to_string(&accounts).unwrap();
    assert!(!output.contains("token A"));
    assert!(!output.contains("files"));
}

#[test]
fn refreshed_session_preserves_label_and_new_credentials() {
    let (_dir, manager) = fixture();
    login(&manager, A, "old");
    let account = manager.save_current(Some("My account")).unwrap();
    login(&manager, A, "rotated");
    manager.save_current(None).unwrap();
    login(&manager, B, "other");
    manager.restore(&account.id).unwrap();
    assert_eq!(
        fs::read(manager.data_dir.join("user.dat")).unwrap(),
        b"rotated"
    );
    assert_eq!(
        manager.accounts().unwrap()[0].display_name.as_deref(),
        Some("My account")
    );
}

#[test]
fn secure_storage_only_login_can_be_saved_and_restored() {
    let (_dir, manager) = fixture();
    login(&manager, A, "legacy");
    fs::remove_file(manager.data_dir.join("user.dat")).unwrap();
    fs::write(
        manager.data_dir.join("ConnectSecureStorage.dat"),
        b"modern secure login",
    )
    .unwrap();
    let saved = manager.save_current(Some("Modern")).unwrap();
    login(&manager, B, "other");
    manager.restore(&saved.id).unwrap();
    assert!(!manager.data_dir.join("user.dat").exists());
    assert_eq!(manager.current_id().as_deref(), Some(saved.id.as_str()));
}

#[test]
fn corrupt_snapshot_is_rejected_before_live_files_change() {
    let (_dir, manager) = fixture();
    login(&manager, A, "good");
    manager.save_current(None).unwrap();
    let path = manager.sessions_dir.join(format!("{A}.json"));
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for (name, content) in [
        ("../escape", "YWJj"),
        ("user.dat", "!invalid-base64"),
        ("user.dat", ""),
    ] {
        let mut corrupt = original.clone();
        corrupt["files"][name] = json!(content);
        fs::write(&path, serde_json::to_vec(&corrupt).unwrap()).unwrap();
        assert!(manager.restore(A).is_err());
        assert_eq!(
            fs::read(manager.data_dir.join("user.dat")).unwrap(),
            b"good"
        );
    }
    let mut wrong = original;
    wrong["account"]["id"] = json!(format!("ubisoft_{B}"));
    fs::write(&path, serde_json::to_vec(&wrong).unwrap()).unwrap();
    assert!(manager.restore(A).is_err());
}

#[test]
fn missing_login_cannot_be_saved_and_removal_keeps_live_login() {
    let (_dir, manager) = fixture();
    assert!(manager.save_current(None).is_err());
    login(&manager, A, "live");
    manager.save_current(None).unwrap();
    manager.remove(A).unwrap();
    assert!(manager.accounts().unwrap().is_empty());
    assert_eq!(
        fs::read(manager.data_dir.join("user.dat")).unwrap(),
        b"live"
    );
}

#[tokio::test]
async fn replacing_catalog_removes_uninstalled_games_and_preserves_other_launchers() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let dir = tempdir().unwrap();
    let mut game = games_from_registry(&format!(
        "HKEY_LOCAL_MACHINE\\Ubisoft\\Launcher\\Installs\\635\n InstallDir REG_SZ {}",
        dir.path().display()
    ))
    .remove(0);
    db.replace_ubisoft_games(&[game.clone()]).await.unwrap();
    game.id = "steam_635".into();
    game.launcher = "steam".into();
    db.upsert_game(&game).await.unwrap();
    db.replace_ubisoft_games(&[]).await.unwrap();
    assert_eq!(db.get_games().await.unwrap(), vec![game]);
}

#[tokio::test]
async fn api_switch_refresh_restart_and_remove_use_saved_identity() {
    let (_dir, manager) = fixture();
    // A dummy client lets the close operation target only this test installation.
    fs::write(manager.launcher_dir.as_ref().unwrap().join("upc.exe"), "").unwrap();
    login(&manager, A, "first");
    manager.save_current(Some("First")).unwrap();
    login(&manager, B, "second");
    manager.save_current(Some("Second")).unwrap();
    let registry = manager.data_dir.join("registry.txt");
    fs::write(&registry, "").unwrap();
    for (key, path) in [
        ("SWCH_UBISOFT_DIR", manager.launcher_dir.as_ref().unwrap()),
        ("SWCH_UBISOFT_DATA_DIR", &manager.data_dir),
        ("SWCH_UBISOFT_SESSIONS_DIR", &manager.sessions_dir),
        ("SWCH_UBISOFT_REGISTRY_FILE", &registry),
    ] {
        std::env::set_var(key, path);
    }
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let app = swch::server::create_app(db.clone());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/ubisoft/switch-account")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"account_id":format!("ubisoft_{A}"),"launch":false}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/ubisoft/library")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let data: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(
        data["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| a["is_active"] == true)
            .count(),
        1
    );
    assert!(!data.to_string().contains("first"));
    // A failed restart must not refresh either saved account with logged-out files.
    let saved_before = fs::read(manager.sessions_dir.join(format!("{A}.json"))).unwrap();
    fs::write(manager.data_dir.join("user.dat"), "not logged in").unwrap();
    fs::write(
        manager
            .launcher_dir
            .as_ref()
            .unwrap()
            .join("logs/launcher_log.txt"),
        format!("User: {A}\n-- Starting Ubisoft Game Launcher --\nStartView.cpp"),
    )
    .unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/ubisoft/sessions/save")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/ubisoft/switch-account")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"account_id":format!("ubisoft_{A}"),"launch":false}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        fs::read(manager.sessions_dir.join(format!("{A}.json"))).unwrap(),
        saved_before
    );
    assert_eq!(
        fs::read(manager.data_dir.join("user.dat")).unwrap(),
        b"first"
    );
    login(&manager, B, "fresh second");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/ubisoft/accounts/prepare")
                .header("content-type", "application/json")
                .body(Body::from("{\"launch\":false}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!manager.data_dir.join("user.dat").exists());
    assert!(manager.accounts().unwrap().iter().all(|a| !a.is_active));
    assert!(db
        .get_accounts()
        .await
        .unwrap()
        .iter()
        .filter(|a| a.launcher == "ubisoft")
        .all(|a| !a.is_active));
    manager.restore(B).unwrap();
    assert_eq!(
        fs::read(manager.data_dir.join("user.dat")).unwrap(),
        b"fresh second"
    );
    // Automatic capture waits for a new authenticated login and completes once.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/ubisoft/accounts/prepare")
                .header("content-type", "application/json")
                .body(Body::from("{\"launch\":false,\"automatic\":true}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let prepared: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let automatic_id = prepared["automatic_session_id"].as_str().unwrap();
    let status_url = format!("/api/ubisoft/auth/status?session_id={automatic_id}");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/ubisoft/auth/status?session_id=wrong")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    login(&manager, A, "automatic token");
    let profile_cache = manager
        .data_dir
        .join("cache/http2/Default/Local Storage/leveldb");
    fs::create_dir_all(&profile_cache).unwrap();
    // The native login is already reported, but its profile arrives afterwards.
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    let waiting = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&status_url)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let waiting: Value =
        serde_json::from_slice(&waiting.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(waiting["status"], "waiting");
    fs::write(
        profile_cache.join("000001.log"),
        json!({"userId":A,"nameOnPlatform":"AutomaticNickname"}).to_string(),
    )
    .unwrap();
    let mut automatic = json!({});
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&status_url)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        automatic =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        if automatic["status"] == "connected" {
            break;
        }
    }
    assert_eq!(automatic["status"], "connected");
    assert_eq!(automatic["account"]["id"], format!("ubisoft_{A}"));
    assert_eq!(automatic["account"]["username"], "AutomaticNickname");
    assert_eq!(automatic["account"]["display_name"], "First"); // Keep the user's label.
    assert!(!automatic.to_string().contains("automatic token"));
    assert!(!automatic.to_string().contains("previous_account_id"));
    let automatic_snapshot = fs::read(manager.sessions_dir.join(format!("{A}.json"))).unwrap();
    fs::write(manager.data_dir.join("user.dat"), "rotated after capture").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    assert_eq!(
        fs::read(manager.sessions_dir.join(format!("{A}.json"))).unwrap(),
        automatic_snapshot
    );
    // Cancel a fresh attempt and recover the previously saved native session.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/ubisoft/accounts/prepare")
                .header("content-type", "application/json")
                .body(Body::from("{\"launch\":false,\"automatic\":true}"))
                .unwrap(),
        )
        .await
        .unwrap();
    let prepared: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let cancel_id = prepared["automatic_session_id"].as_str().unwrap();
    assert_ne!(cancel_id, automatic_id);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/ubisoft/auth/cancel")
                .header("content-type", "application/json")
                .body(Body::from(json!({"session_id":cancel_id}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        fs::read(manager.data_dir.join("user.dat")).unwrap(),
        b"rotated after capture"
    );
    let second_snapshot = fs::read(manager.sessions_dir.join(format!("{B}.json"))).unwrap();
    login(&manager, B, "must not be auto-saved after cancellation");
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    assert_eq!(
        fs::read(manager.sessions_dir.join(format!("{B}.json"))).unwrap(),
        second_snapshot
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/ubisoft/accounts/remove")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"account_id":format!("ubisoft_{A}")}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(manager.accounts().unwrap().len(), 1);
    assert!(!db
        .get_accounts()
        .await
        .unwrap()
        .iter()
        .any(|a| a.id == format!("ubisoft_{A}")));
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/ubisoft/switch-account")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"account_id":"../escape","launch":false}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    for key in [
        "SWCH_UBISOFT_DIR",
        "SWCH_UBISOFT_DATA_DIR",
        "SWCH_UBISOFT_SESSIONS_DIR",
        "SWCH_UBISOFT_REGISTRY_FILE",
    ] {
        std::env::remove_var(key);
    }
}
