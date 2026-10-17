use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

use swch::launchers::epic::EpicAuthStore;
use swch::launchers::minecraft::MinecraftManager;
use swch::launchers::roblox::RobloxManager;
use swch::modules::ModuleManager;
use swch::server::updater::{
    find_best_asset, is_newer_version, parse_semver, ReleaseAsset, UpdateManager,
};
use swch::server::{create_app_with_state, AppState};
use swch::storage::Database;

#[test]
fn test_semver_parsing() {
    assert_eq!(parse_semver("1.2.3"), (1, 2, 3));
    assert_eq!(parse_semver("v1.2.3"), (1, 2, 3));
    assert_eq!(parse_semver("10.20.30"), (10, 20, 30));
    assert_eq!(parse_semver("1.0"), (1, 0, 0));
    assert_eq!(parse_semver("2"), (2, 0, 0));
    assert_eq!(parse_semver("invalid"), (0, 0, 0));
}

#[test]
fn test_version_comparisons() {
    // Newer version detection
    assert!(is_newer_version("1.1.1", "v1.2.0"));
    assert!(is_newer_version("1.1.1", "1.2.0"));
    assert!(is_newer_version("0.1.0", "v1.0.0"));
    assert!(is_newer_version("1.2.0", "v1.2.1"));
    assert!(is_newer_version("1.2.0", "2.0.0"));
    assert!(is_newer_version("0.9.9", "1.0.0"));

    // Same or older versions
    assert!(!is_newer_version("1.2.0", "v1.2.0"));
    assert!(!is_newer_version("1.2.0", "1.2.0"));
    assert!(!is_newer_version("1.2.1", "v1.2.0"));
    assert!(!is_newer_version("2.0.0", "v1.9.9"));
    assert!(!is_newer_version("1.1.1", "v1.1.1"));
}

#[test]
fn test_asset_matching() {
    let assets = vec![
        ReleaseAsset {
            name: "swch-windows-x86_64.zip".to_string(),
            browser_download_url: "https://example.com/win.zip".to_string(),
            size: 1000,
        },
        ReleaseAsset {
            name: "swch-macos-arm64.zip".to_string(),
            browser_download_url: "https://example.com/mac-arm.zip".to_string(),
            size: 2000,
        },
        ReleaseAsset {
            name: "swch-macos-x86_64.zip".to_string(),
            browser_download_url: "https://example.com/mac-intel.zip".to_string(),
            size: 2500,
        },
        ReleaseAsset {
            name: "swch-linux-x86_64.tar.gz".to_string(),
            browser_download_url: "https://example.com/linux.tar.gz".to_string(),
            size: 1800,
        },
    ];

    // macOS ARM64
    let mac_arm = find_best_asset(&assets, "macos", "aarch64");
    assert!(mac_arm.is_some());
    assert_eq!(mac_arm.unwrap().name, "swch-macos-arm64.zip");

    // macOS Intel
    let mac_intel = find_best_asset(&assets, "macos", "x86_64");
    assert!(mac_intel.is_some());
    assert_eq!(mac_intel.unwrap().name, "swch-macos-x86_64.zip");

    // Windows
    let win = find_best_asset(&assets, "windows", "x86_64");
    assert!(win.is_some());
    assert_eq!(win.unwrap().name, "swch-windows-x86_64.zip");

    // Linux
    let linux = find_best_asset(&assets, "linux", "x86_64");
    assert!(linux.is_some());
    assert_eq!(linux.unwrap().name, "swch-linux-x86_64.tar.gz");
}

#[tokio::test]
async fn test_update_status_endpoint() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let epic_store = Arc::new(EpicAuthStore::default_store(db.clone()));
    let module_manager = Arc::new(ModuleManager::new(db.clone()));
    let roblox_manager = Arc::new(RobloxManager::new(db.clone()));
    let minecraft_manager = Arc::new(MinecraftManager::new(db.clone()));
    let update_manager = Arc::new(UpdateManager::new());

    let app = create_app_with_state(AppState {
        db: db.clone(),
        epic_store,
        module_manager,
        roblox_manager,
        minecraft_manager,
        update_manager,
    });

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/update/status")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["phase"], "idle");
    assert_eq!(json["progress_percent"], 0);
    assert_eq!(json["downloaded_bytes"], 0);
    assert!(json["total_bytes"].is_null());
    assert!(json["download_percent"].is_null());
    assert_eq!(json["bytes_per_second"], 0);
    assert!(json["eta_seconds"].is_null());
}

#[test]
fn test_app_bundle_detection() {
    use swch::server::updater::find_enclosing_app_bundle;
    use std::path::Path;

    // Inside macOS bundle
    let p1 = Path::new("/Applications/swch.app/Contents/MacOS/swch-server");
    assert_eq!(find_enclosing_app_bundle(p1), Some(Path::new("/Applications/swch.app").to_path_buf()));

    let p2 = Path::new("/Users/john/Downloads/swch.app/Contents/MacOS/swch");
    assert_eq!(find_enclosing_app_bundle(p2), Some(Path::new("/Users/john/Downloads/swch.app").to_path_buf()));

    // Standalone binary (not in bundle)
    let p3 = Path::new("/usr/local/bin/swch");
    assert_eq!(find_enclosing_app_bundle(p3), None);

    let p4 = Path::new("/Users/nazarzukov/swch2/target_build/release/swch");
    assert_eq!(find_enclosing_app_bundle(p4), None);
}

#[test]
fn test_find_app_and_binary_in_dir() {
    use swch::server::updater::{find_app_in_dir, find_binary_in_dir};
    use tempfile::tempdir;

    let tmp = tempdir().unwrap();
    let nested_app = tmp.path().join("subfolder/swch.app");
    std::fs::create_dir_all(&nested_app).unwrap();

    let found_app = find_app_in_dir(tmp.path());
    assert!(found_app.is_some());
    assert_eq!(found_app.unwrap().file_name().unwrap(), "swch.app");

    let bin_path = tmp.path().join("subfolder/swch");
    std::fs::write(&bin_path, b"dummy").unwrap();

    let found_bin = find_binary_in_dir(tmp.path(), "swch");
    assert!(found_bin.is_some());
    assert_eq!(found_bin.unwrap().file_name().unwrap(), "swch");
}

#[tokio::test]
async fn test_update_check_endpoint() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let epic_store = Arc::new(EpicAuthStore::default_store(db.clone()));
    let module_manager = Arc::new(ModuleManager::new(db.clone()));
    let roblox_manager = Arc::new(RobloxManager::new(db.clone()));
    let minecraft_manager = Arc::new(MinecraftManager::new(db.clone()));
    let update_manager = Arc::new(UpdateManager::new());

    let app = create_app_with_state(AppState {
        db: db.clone(),
        epic_store,
        module_manager,
        roblox_manager,
        minecraft_manager,
        update_manager,
    });

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/update/check")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    println!("Response status: {}, body: {:?}", status, json);
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["current_version"], env!("CARGO_PKG_VERSION"));
    assert!(json["target_platform"].is_string());
}

#[test]
fn test_intel_mac_does_not_pick_arm64_asset() {
    let arm_only_assets = vec![ReleaseAsset {
        name: "swch-macos-arm64.zip".to_string(),
        browser_download_url: "https://example.com/mac-arm.zip".to_string(),
        size: 2000,
    }];

    // Intel Mac should NOT pick arm64 asset
    let intel_match = find_best_asset(&arm_only_assets, "macos", "x86_64");
    assert!(intel_match.is_none());

    // Apple Silicon Mac SHOULD pick arm64 asset
    let arm_match = find_best_asset(&arm_only_assets, "macos", "arm64");
    assert!(arm_match.is_some());
    assert_eq!(arm_match.unwrap().name, "swch-macos-arm64.zip");
}

#[test]
fn test_windows_asset_naming() {
    let win_assets = vec![
        ReleaseAsset {
            name: "swch-windows-amd64.zip".to_string(),
            browser_download_url: "https://example.com/win-amd64.zip".to_string(),
            size: 2000,
        },
        ReleaseAsset {
            name: "swch-windows-x86_64.zip".to_string(),
            browser_download_url: "https://example.com/win-x86_64.zip".to_string(),
            size: 2000,
        },
    ];

    let win_match = find_best_asset(&win_assets, "windows", "x86_64");
    assert!(win_match.is_some());
}

#[test]
fn test_windows_companion_gui_and_loader_presence() {
    use swch::server::updater::find_binary_in_dir;
    use tempfile::tempdir;

    let tmp = tempdir().unwrap();
    let package_dir = tmp.path().join("package");
    std::fs::create_dir_all(&package_dir).unwrap();

    let gui_path = package_dir.join(if cfg!(windows) { "swch.exe" } else { "swch.exe" });
    let server_path = package_dir.join(if cfg!(windows) { "swch-server.exe" } else { "swch-server.exe" });
    let loader_path = package_dir.join("WebView2Loader.dll");

    std::fs::write(&gui_path, b"gui-dummy").unwrap();
    std::fs::write(&server_path, b"server-dummy").unwrap();
    std::fs::write(&loader_path, b"loader-dummy").unwrap();

    // On windows or non-windows, finding binary with .exe extension works with find_binary_in_dir
    let found_server = find_binary_in_dir(tmp.path(), if cfg!(windows) { "swch-server" } else { "swch-server.exe" });
    assert!(found_server.is_some());

    let found_gui = find_binary_in_dir(tmp.path(), if cfg!(windows) { "swch" } else { "swch.exe" });
    assert!(found_gui.is_some());
}

#[test]
fn test_copy_and_move_helpers() {
    use swch::server::updater::{copy_dir_all, move_or_copy_dir};
    use tempfile::tempdir;

    let tmp = tempdir().unwrap();
    let src = tmp.path().join("src_dir");
    let dst = tmp.path().join("dst_dir");
    std::fs::create_dir_all(src.join("nested")).unwrap();
    std::fs::write(src.join("nested/test.txt"), b"hello world").unwrap();

    // Test copy_dir_all
    copy_dir_all(&src, &dst).unwrap();
    assert!(dst.join("nested/test.txt").exists());
    assert_eq!(std::fs::read(dst.join("nested/test.txt")).unwrap(), b"hello world");

    // Test move_or_copy_dir
    let final_dst = tmp.path().join("final_dst");
    move_or_copy_dir(&dst, &final_dst).unwrap();
    assert!(final_dst.join("nested/test.txt").exists());
    assert!(!dst.exists());
}

#[test]
fn test_replace_executable_helper() {
    use swch::server::updater::replace_executable;
    use tempfile::tempdir;

    let tmp = tempdir().unwrap();
    let current_exe = tmp.path().join("test_bin");
    let new_bin = tmp.path().join("new_bin");

    std::fs::write(&current_exe, b"version 1").unwrap();
    std::fs::write(&new_bin, b"version 2").unwrap();

    replace_executable(&current_exe, &new_bin).unwrap();
    assert_eq!(std::fs::read(&current_exe).unwrap(), b"version 2");
}

