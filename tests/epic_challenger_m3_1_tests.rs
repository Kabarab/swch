//! Adversarial Empirical Challenger Test Suite for Milestone 3 (Challenger 1)
//! Focus: Launch Argument Ordering, Secret Masking Boundaries, Display/Debug Redaction, Process Execution Modes.

mod common;

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tempfile::tempdir;
use tower::ServiceExt;

use common::mock_server::MockEpicServer;
use swch::launchers::epic::client::EpicClient;
use swch::launchers::epic::keyring::MemoryCredentialStore;
use swch::launchers::epic::launch::*;
use swch::launchers::epic::models::*;
use swch::launchers::epic::EpicAuthStore;
use swch::parsers::EpicManifestItem;
use swch::server::{create_app_with_state, AppState};
use swch::storage::Database;

// ============================================================================
// 1. Unreal / Epic Launch Parameter Builder Ordering Invariants
// ============================================================================

#[test]
fn test_adversarial_ordering_mandatory_six_arguments_first() {
    let exchange_code = "ex_code_adversarial_998877";
    let app_name = "SugarGame";
    let account_id = "acc_order_123";
    let display_name = "OrderTester";
    let namespace = "sandbox_xyz";

    let args = build_launch_parameters(
        exchange_code,
        app_name,
        account_id,
        display_name,
        namespace,
        Some("en"),
        None,
    );

    assert!(args.len() >= 6, "Must produce at least 6 arguments");
    assert_eq!(args[0], "-AUTH_LOGIN=unused", "Arg 0 must be -AUTH_LOGIN=unused");
    assert_eq!(
        args[1],
        format!("-AUTH_PASSWORD={}", exchange_code),
        "Arg 1 must be -AUTH_PASSWORD=<code_or_mask>"
    );
    assert_eq!(args[2], "-AUTH_TYPE=exchangecode", "Arg 2 must be -AUTH_TYPE=exchangecode");
    assert_eq!(args[3], format!("-epicapp={}", app_name), "Arg 3 must be -epicapp=<appName>");
    assert_eq!(args[4], "-epicenv=Prod", "Arg 4 must be -epicenv=Prod");
    assert_eq!(args[5], "-EpicPortal", "Arg 5 must be -EpicPortal");

    // Check context arguments follow immediately
    assert_eq!(args[6], format!("-epicusername={}", display_name));
    assert_eq!(args[7], format!("-epicuserid={}", account_id));
    assert_eq!(args[8], "-epiclocale=en");
    assert_eq!(args[9], format!("-epicsandboxid={}", namespace));
}

#[test]
fn test_adversarial_ordering_hijack_attempt_via_custom_args() {
    // Attempt to inject colliding/counterfeit Unreal flags via custom arguments
    let counterfeit_args = vec![
        "-AUTH_LOGIN=hacker_login".to_string(),
        "-AUTH_PASSWORD=counterfeit_token".to_string(),
        "-AUTH_TYPE=user_password".to_string(),
        "-epicapp=TrojanApp".to_string(),
        "-epicenv=Development".to_string(),
        "-EpicPortal=false".to_string(),
        "-epicusername=impersonator".to_string(),
        "-epicuserid=root_0000".to_string(),
        "-epiclocale=adversarial".to_string(),
        "-epicsandboxid=malicious_sandbox".to_string(),
    ];

    let exchange_code = "legitimate_exchange_token";
    let args = build_launch_parameters(
        exchange_code,
        "AuthenticApp",
        "acc_real_user",
        "AuthenticGamer",
        "real_namespace",
        Some("en"),
        Some(&counterfeit_args),
    );

    assert_eq!(args.len(), 20, "Should contain 10 base args + 10 custom args");

    // The primary 10 arguments MUST be preserved in exact positions 0..9
    assert_eq!(args[0], "-AUTH_LOGIN=unused");
    assert_eq!(args[1], format!("-AUTH_PASSWORD={}", exchange_code));
    assert_eq!(args[2], "-AUTH_TYPE=exchangecode");
    assert_eq!(args[3], "-epicapp=AuthenticApp");
    assert_eq!(args[4], "-epicenv=Prod");
    assert_eq!(args[5], "-EpicPortal");
    assert_eq!(args[6], "-epicusername=AuthenticGamer");
    assert_eq!(args[7], "-epicuserid=acc_real_user");
    assert_eq!(args[8], "-epiclocale=en");
    assert_eq!(args[9], "-epicsandboxid=real_namespace");

    // The injected args must be appended at indices 10..19 without reordering the prefix
    for (i, extra) in counterfeit_args.iter().enumerate() {
        assert_eq!(&args[10 + i], extra);
    }
}

#[test]
fn test_adversarial_ordering_with_empty_custom_args_slice() {
    let empty_slice: [String; 0] = [];
    let args = build_launch_parameters(
        "token_1",
        "App",
        "acc",
        "User",
        "ns",
        None,
        Some(&empty_slice),
    );
    assert_eq!(args.len(), 10, "Empty slice must not produce dummy empty strings");
}

#[test]
fn test_adversarial_ordering_with_various_cli_flag_styles() {
    let complex_flags = vec![
        "-novid".to_string(),
        "-fullscreen".to_string(),
        "--windowed".to_string(),
        "/DEBUG".to_string(),
        "+connect 192.168.1.1:27015".to_string(),
        "PARAM_WITHOUT_DASH=VALUE".to_string(),
        "".to_string(),
    ];

    let args = build_launch_parameters(
        "token_2",
        "App",
        "acc",
        "User",
        "ns",
        None,
        Some(&complex_flags),
    );

    assert_eq!(args.len(), 17);
    assert_eq!(args[0], "-AUTH_LOGIN=unused");
    assert_eq!(args[10], "-novid");
    assert_eq!(args[14], "+connect 192.168.1.1:27015");
    assert_eq!(args[15], "PARAM_WITHOUT_DASH=VALUE");
    assert_eq!(args[16], "");
}

// ============================================================================
// 2. Secret Masking & Redaction Boundaries
// ============================================================================

#[test]
fn test_adversarial_masking_empty_password_flag() {
    let empty_arg = "-AUTH_PASSWORD=";
    assert_eq!(mask_arg(empty_arg), "-AUTH_PASSWORD=********");

    let args = vec![empty_arg.to_string()];
    let masked = mask_launch_arguments(&args);
    assert_eq!(masked[0], "-AUTH_PASSWORD=********");
}

#[test]
fn test_adversarial_masking_case_insensitivity_comprehensive() {
    let variations = [
        "-AUTH_PASSWORD=token123",
        "-auth_password=token123",
        "-Auth_Password=token123",
        "-aUtH_pAsSwOrD=token123",
        "-AuTh_PaSsWoRd=token123",
        "-AUTH_password=token123",
        "-auth_PASSWORD=token123",
    ];

    for var in &variations {
        let masked = mask_arg(var);
        assert_eq!(
            masked, "-AUTH_PASSWORD=********",
            "Failed for case variation: {}",
            var
        );
    }
}

#[test]
fn test_adversarial_masking_epicovt_case_insensitivity_comprehensive() {
    let variations = [
        "-EPICOVT=super_secret_ovt",
        "-epicovt=super_secret_ovt",
        "-EpicOvt=super_secret_ovt",
        "-ePiCoVt=super_secret_ovt",
        "-EPICovt=super_secret_ovt",
    ];

    for var in &variations {
        let masked = mask_arg(var);
        assert_eq!(
            masked, "-epicovt=[REDACTED]",
            "Failed for case variation: {}",
            var
        );
    }
}

#[test]
fn test_adversarial_masking_embedded_equals_quotes_and_newlines() {
    let eq_token = "-AUTH_PASSWORD=key=value=with=multiple=equals";
    assert_eq!(mask_arg(eq_token), "-AUTH_PASSWORD=********");

    let quote_token = "-AUTH_PASSWORD=\"token_with_quotes\"";
    assert_eq!(mask_arg(quote_token), "-AUTH_PASSWORD=********");

    let ws_token = "-AUTH_PASSWORD=token with spaces\tand\nnewlines\r";
    assert_eq!(mask_arg(ws_token), "-AUTH_PASSWORD=********");
}

#[test]
fn test_adversarial_masking_multibyte_utf8_token() {
    let utf8_token = "-AUTH_PASSWORD=СекретныйТокен_秘密_🔑_🚀";
    assert_eq!(mask_arg(utf8_token), "-AUTH_PASSWORD=********");

    let args = vec![utf8_token.to_string()];
    let masked = mask_launch_arguments(&args);
    assert_eq!(masked[0], "-AUTH_PASSWORD=********");
}

#[test]
fn test_adversarial_masking_stress_huge_token() {
    let huge_secret = "A".repeat(100_000);
    let arg = format!("-AUTH_PASSWORD={}", huge_secret);

    let start = std::time::Instant::now();
    let masked = mask_arg(&arg);
    let elapsed = start.elapsed();

    assert_eq!(masked, "-AUTH_PASSWORD=********");
    assert!(
        elapsed.as_millis() < 50,
        "Masking huge token took too long: {:?}",
        elapsed
    );
}

#[test]
fn test_adversarial_masking_near_miss_arguments_not_corrupted() {
    let non_sensitive = [
        "-AUTH_TYPE=exchangecode",
        "-AUTH_LOGIN=unused",
        "-AUTH_PASSWORD_WITHOUT_EQUALS",
        "-AUTH_PASS=truncated",
        "-epicapp=Sugar",
        "-epicenv=Prod",
        "-epiclocale=en",
        "-epicsandboxid=fn",
        "-epicovt",
        "-EPICOVT",
        "-password=not_auth_password",
    ];

    for arg in &non_sensitive {
        assert_eq!(mask_arg(arg), *arg, "Near miss argument should not be modified: {}", arg);
    }
}

#[test]
fn test_adversarial_redacted_launch_args_display_and_debug_guarantee() {
    let exchange_code = "SUPER_SECRET_ONE_TIME_EXCHANGE_CODE_987654321";
    let args = vec![
        "-AUTH_LOGIN=unused".to_string(),
        format!("-AUTH_PASSWORD={}", exchange_code),
        "-AUTH_TYPE=exchangecode".to_string(),
        "-epicovt=ANOTHER_SUPER_SECRET_OVT_999999".to_string(),
    ];

    let redacted = RedactedLaunchArgs(&args);

    let display_str = format!("{}", redacted);
    assert!(
        !display_str.contains(exchange_code),
        "Raw exchange code leaked in Display: {}",
        display_str
    );
    assert!(
        !display_str.contains("ANOTHER_SUPER_SECRET_OVT_999999"),
        "Raw OVT token leaked in Display: {}",
        display_str
    );
    assert!(display_str.contains("-AUTH_PASSWORD=********"));
    assert!(display_str.contains("-epicovt=[REDACTED]"));

    let debug_str = format!("{:?}", redacted);
    assert!(
        !debug_str.contains(exchange_code),
        "Raw exchange code leaked in Debug: {}",
        debug_str
    );
    assert!(
        !debug_str.contains("ANOTHER_SUPER_SECRET_OVT_999999"),
        "Raw OVT token leaked in Debug: {}",
        debug_str
    );
    assert!(debug_str.contains("-AUTH_PASSWORD=********"));
    assert!(debug_str.contains("-epicovt=[REDACTED]"));
}

#[test]
fn test_adversarial_launch_command_redacted_helpers() {
    let exchange_code = "HIGHLY_CONFIDENTIAL_TOKEN_654321";
    let cmd = LaunchCommand {
        executable_path: PathBuf::from("/Applications/Game.app/Contents/MacOS/Game"),
        args: vec![
            "-AUTH_LOGIN=unused".to_string(),
            format!("-AUTH_PASSWORD={}", exchange_code),
            "-EpicPortal".to_string(),
        ],
        display_args: vec![
            "-AUTH_LOGIN=unused".to_string(),
            "-AUTH_PASSWORD=********".to_string(),
            "-EpicPortal".to_string(),
        ],
        working_dir: PathBuf::from("/Applications/Game.app/Contents/MacOS"),
        env: HashMap::new(),
    };

    let display_cmd = cmd.display_command_line();
    assert!(!display_cmd.contains(exchange_code));
    assert!(display_cmd.contains("-AUTH_PASSWORD=********"));

    let redacted_cmd = cmd.redacted_command_line();
    assert!(!redacted_cmd.contains(exchange_code));
    assert!(redacted_cmd.contains("-AUTH_PASSWORD=********"));

    let debug_redacted_args = format!("{:?}", cmd.redacted_args());
    assert!(!debug_redacted_args.contains(exchange_code));
    assert!(debug_redacted_args.contains("-AUTH_PASSWORD=********"));
}

// ============================================================================
// 3. Locale Resolution & Boundary Cases
// ============================================================================

#[test]
fn test_adversarial_locale_resolution_variations() {
    let test_cases = [
        (None, "-epiclocale=en"),
        (Some(""), "-epiclocale=en"),
        (Some("   "), "-epiclocale=en"),
        (Some("\t\n\r "), "-epiclocale=en"),
        (Some("en-US"), "-epiclocale=en-US"),
        (Some("ru-RU"), "-epiclocale=ru-RU"),
        (Some("  ja-JP  "), "-epiclocale=ja-JP"),
        (Some("zh-Hans-CN"), "-epiclocale=zh-Hans-CN"),
    ];

    for (input, expected) in test_cases {
        let args = build_launch_parameters("code", "App", "acc", "User", "ns", input, None);
        assert!(
            args.contains(&expected.to_string()),
            "Failed for locale input '{:?}': expected '{}' in {:?}",
            input,
            expected,
            args
        );
    }
}

// ============================================================================
// 4. Prepare Launch Command Integrity & Working Directory Resolution
// ============================================================================

fn sample_test_manifest(is_incomplete: bool, is_dlc: bool) -> EpicManifestItem {
    EpicManifestItem {
        format_version: Some(0),
        app_name: if is_dlc { "DlcApp".to_string() } else { "MainGameApp".to_string() },
        catalog_item_id: "cat_item_1".to_string(),
        catalog_namespace: "ns_test_123".to_string(),
        display_name: "Test Game Title".to_string(),
        installation_guid: "guid_123".to_string(),
        install_location: "/tmp/swch_test_games/game_folder".to_string(),
        install_size: 1048576,
        launch_executable: "Binaries/Win64/Game.exe".to_string(),
        launch_command: None,
        main_game_app_name: if is_dlc { Some("MainGameApp".to_string()) } else { None },
        mandatory_app_folder_name: None,
        is_incomplete_install: Some(is_incomplete),
        can_run_offline: Some(true),
        requires_auth: Some(true),
        app_version_string: Some("1.0.0".to_string()),
    }
}

#[test]
fn test_adversarial_prepare_launch_incomplete_install_rejected() {
    let manifest = sample_test_manifest(true, false);
    let res = prepare_launch_command(&manifest, "token", "acc_1", "User", None, None);

    match res {
        Err(LaunchError::IncompleteInstall) => {}
        other => panic!("Expected IncompleteInstall error, got {:?}", other),
    }
}

#[test]
fn test_adversarial_prepare_launch_dlc_rejection() {
    let manifest = sample_test_manifest(false, true);
    let res = prepare_launch_command(&manifest, "token", "acc_1", "User", None, None);

    match res {
        Err(LaunchError::CannotLaunchDlc { app_name, main_game }) => {
            assert_eq!(app_name, "DlcApp");
            assert_eq!(main_game, "MainGameApp");
        }
        other => panic!("Expected CannotLaunchDlc error, got {:?}", other),
    }
}

#[test]
fn test_adversarial_prepare_launch_environment_variables_populated() {
    let manifest = sample_test_manifest(false, false);
    let cmd = prepare_launch_command(
        &manifest,
        "token_env_test",
        "acc_env_456",
        "EnvGamer",
        Some("de-DE"),
        None,
    )
    .expect("Prepare launch should succeed");

    assert_eq!(cmd.env.get("EPIC_APP_NAME"), Some(&"MainGameApp".to_string()));
    assert_eq!(cmd.env.get("EPIC_NAMESPACE"), Some(&"ns_test_123".to_string()));
    assert_eq!(cmd.env.get("EPIC_ACCOUNT_ID"), Some(&"acc_env_456".to_string()));
    assert_eq!(cmd.env.get("EPIC_DISPLAY_NAME"), Some(&"EnvGamer".to_string()));
}

#[test]
fn test_adversarial_prepare_launch_working_dir_existing_file_vs_fallback() {
    let temp = tempdir().unwrap();
    let install_dir = temp.path().join("FakeGame");
    let bin_dir = install_dir.join("Binaries/Win64");
    fs::create_dir_all(&bin_dir).unwrap();
    let exe_file = bin_dir.join("Game.exe");
    File::create(&exe_file).unwrap().write_all(b"fake_bin").unwrap();

    let mut manifest = sample_test_manifest(false, false);
    manifest.install_location = install_dir.to_string_lossy().to_string();
    manifest.launch_executable = "Binaries/Win64/Game.exe".to_string();

    let cmd = prepare_launch_command(&manifest, "tok", "acc", "User", None, None).unwrap();
    assert_eq!(
        cmd.working_dir, bin_dir,
        "Working dir should be parent of existing executable"
    );

    // Non-existent executable file -> working_dir falls back to install_location
    manifest.launch_executable = "NonExistent/Binary.exe".to_string();
    let cmd2 = prepare_launch_command(&manifest, "tok", "acc", "User", None, None).unwrap();
    assert_eq!(
        cmd2.working_dir, install_dir,
        "Working dir should fall back to install_location when exe file doesn't exist"
    );
}

// ============================================================================
// 5. macOS Application Bundle Mach-O Resolution
// ============================================================================

#[test]
#[cfg(target_os = "macos")]
fn test_adversarial_macos_bundle_resolution_with_valid_plist() {
    let temp = tempdir().unwrap();
    let bundle_dir = temp.path().join("CivilizationVI.app");
    let macos_dir = bundle_dir.join("Contents/MacOS");
    fs::create_dir_all(&macos_dir).unwrap();

    let plist_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>Civ6SubMachO</string>
    <key>CFBundleIdentifier</key>
    <string>com.aspyr.civ6</string>
</dict>
</plist>"#;
    fs::write(bundle_dir.join("Contents/Info.plist"), plist_content).unwrap();

    let macho_file = macos_dir.join("Civ6SubMachO");
    File::create(&macho_file).unwrap().write_all(b"\xCF\xFA\xED\xFE").unwrap();

    let mut manifest = sample_test_manifest(false, false);
    manifest.install_location = bundle_dir.to_string_lossy().to_string();
    manifest.launch_executable = "".to_string();

    let resolved = manifest.resolve_executable_path();
    assert_eq!(resolved, macho_file, "Must resolve to Mach-O binary specified in CFBundleExecutable");
}

#[test]
#[cfg(target_os = "macos")]
fn test_adversarial_macos_bundle_resolution_with_whitespace_in_plist() {
    let temp = tempdir().unwrap();
    let bundle_dir = temp.path().join("WhitespaceGame.app");
    let macos_dir = bundle_dir.join("Contents/MacOS");
    fs::create_dir_all(&macos_dir).unwrap();

    let plist_content = r#"<dict>
    <key>CFBundleExecutable</key>
    <string>   TrimmedBinary   </string>
</dict>"#;
    fs::write(bundle_dir.join("Contents/Info.plist"), plist_content).unwrap();

    let macho_file = macos_dir.join("TrimmedBinary");
    File::create(&macho_file).unwrap().write_all(b"binary").unwrap();

    let mut manifest = sample_test_manifest(false, false);
    manifest.install_location = bundle_dir.to_string_lossy().to_string();
    manifest.launch_executable = "".to_string();

    let resolved = manifest.resolve_executable_path();
    assert_eq!(resolved, macho_file, "Must trim whitespace from CFBundleExecutable value");
}

#[test]
#[cfg(target_os = "macos")]
fn test_adversarial_macos_bundle_resolution_fallback_to_app_stem() {
    let temp = tempdir().unwrap();
    let bundle_dir = temp.path().join("StemGame.app");
    let macos_dir = bundle_dir.join("Contents/MacOS");
    fs::create_dir_all(&macos_dir).unwrap();

    let stem_binary = macos_dir.join("StemGame");
    File::create(&stem_binary).unwrap().write_all(b"binary").unwrap();

    let mut manifest = sample_test_manifest(false, false);
    manifest.install_location = bundle_dir.to_string_lossy().to_string();
    manifest.launch_executable = "".to_string();

    let resolved = manifest.resolve_executable_path();
    assert_eq!(resolved, stem_binary, "Must fall back to bundle stem binary name");
}

// ============================================================================
// 6. Process Execution & Simulation Modes
// ============================================================================

#[test]
fn test_adversarial_launched_process_simulated_properties() {
    let proc = LaunchedProcess::new_simulated(42424, "fake_cmd".to_string());
    assert_eq!(proc.pid(), 42424);
    assert!(proc.is_simulated());
    assert_eq!(proc.display_command(), "fake_cmd");

    let mut proc_mut = proc;
    assert!(proc_mut.try_wait().unwrap().is_none());
    let exit = proc_mut.wait().unwrap();
    assert!(exit.success());
    assert!(proc_mut.kill().is_ok());
    assert!(proc_mut.take_child().is_none());
}

#[test]
fn test_adversarial_execute_game_dry_run_simulation() {
    let cmd = LaunchCommand {
        executable_path: PathBuf::from("/nonexistent/binary_path"),
        args: vec!["-AUTH_LOGIN=unused".to_string()],
        display_args: vec!["-AUTH_LOGIN=unused".to_string()],
        working_dir: PathBuf::from("/nonexistent"),
        env: HashMap::new(),
    };

    let options = LaunchOptions::simulated();
    let proc = execute_game_with_options(&cmd, &options).expect("Dry-run execution must succeed");
    assert_eq!(proc.pid(), 42424);
    assert!(proc.is_simulated());
}

#[test]
fn test_adversarial_execute_game_simulate_env_var_override() {
    let cmd = LaunchCommand {
        executable_path: PathBuf::from("/absolutely/nonexistent/game_binary_xyz"),
        args: vec!["-AUTH_LOGIN=unused".to_string()],
        display_args: vec!["-AUTH_LOGIN=unused".to_string()],
        working_dir: PathBuf::from("/absolutely"),
        env: HashMap::new(),
    };

    std::env::set_var("SWCH_EPIC_SIMULATE_LAUNCH", "1");
    let proc_result = execute_game(&cmd);
    std::env::remove_var("SWCH_EPIC_SIMULATE_LAUNCH");

    let proc = proc_result.expect("SWCH_EPIC_SIMULATE_LAUNCH should trigger simulation mode");
    assert_eq!(proc.pid(), 42424);
    assert!(proc.is_simulated());
}

#[test]
fn test_adversarial_execute_game_nonexistent_executable_fails() {
    std::env::remove_var("SWCH_EPIC_SIMULATE_LAUNCH");
    let cmd = LaunchCommand {
        executable_path: PathBuf::from("/absolutely/nonexistent/game_binary_xyz"),
        args: vec!["-AUTH_LOGIN=unused".to_string()],
        display_args: vec!["-AUTH_LOGIN=unused".to_string()],
        working_dir: PathBuf::from("/absolutely"),
        env: HashMap::new(),
    };

    let options = LaunchOptions::default();
    let res = execute_game_with_options(&cmd, &options);
    assert!(
        matches!(res, Err(LaunchError::ExecutableNotFound(_))),
        "Expected ExecutableNotFound"
    );
}

#[test]
#[cfg(unix)]
fn test_adversarial_execute_game_live_process_spawn() {
    let echo_path = PathBuf::from("/bin/echo");
    if !echo_path.exists() {
        return;
    }

    let cmd = LaunchCommand {
        executable_path: echo_path,
        args: vec!["HelloFromChallenger".to_string()],
        display_args: vec!["HelloFromChallenger".to_string()],
        working_dir: PathBuf::from("/tmp"),
        env: HashMap::new(),
    };

    let options = LaunchOptions {
        dry_run: false,
        stdin: StdioMode::Null,
        stdout: StdioMode::Piped,
        stderr: StdioMode::Piped,
        extra_env: HashMap::new(),
    };

    let mut proc = execute_game_with_options(&cmd, &options).expect("Live echo spawn should succeed");
    assert!(!proc.is_simulated());
    assert!(proc.pid() > 0);

    let status = proc.wait().expect("Wait on child should succeed");
    assert!(status.success());
}

// ============================================================================
// 7. Error Type Conversions & Invariants
// ============================================================================

#[test]
fn test_adversarial_launch_error_conversions() {
    let err_missing = LaunchError::MissingAccount;
    let epic_err: EpicError = err_missing.into();
    assert!(matches!(epic_err, EpicError::NoActiveAccount));

    let back_err: LaunchError = epic_err.into();
    assert!(matches!(back_err, LaunchError::MissingAccount));

    let auth_msg = "Token expired".to_string();
    let err_auth = LaunchError::ExchangeTokenError(auth_msg.clone());
    let epic_err_auth: EpicError = err_auth.into();
    assert!(matches!(epic_err_auth, EpicError::Auth(_)));

    let back_auth: LaunchError = epic_err_auth.into();
    assert!(matches!(back_auth, LaunchError::ExchangeTokenError(_)));

    let err_dlc = LaunchError::CannotLaunchDlc {
        app_name: "DLC".to_string(),
        main_game: "Game".to_string(),
    };
    let epic_dlc: EpicError = err_dlc.into();
    assert!(matches!(epic_dlc, EpicError::Database(_)));
}

// ============================================================================
// 8. EpicAuthStore & REST API Launch Integration
// ============================================================================

async fn setup_challenger_store() -> (MockEpicServer, Database, Arc<EpicAuthStore>) {
    let mock = MockEpicServer::start().await;
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let keyring = Arc::new(MemoryCredentialStore::new());
    let client = Arc::new(EpicClient::from_router(mock.router()));
    let store = Arc::new(EpicAuthStore::new(db.clone(), keyring, client));
    (mock, db, store)
}

#[tokio::test]
async fn test_adversarial_store_launch_empty_app_name_rejected() {
    let (_, _, store) = setup_challenger_store().await;
    let res = store
        .launch_game(Some("acc_1"), "   ", None, None)
        .await;
    assert!(matches!(res, Err(EpicError::Database(_))));
}

#[tokio::test]
async fn test_adversarial_store_launch_no_active_account_rejected() {
    let (_, _, store) = setup_challenger_store().await;
    let res = store.launch_game(None, "Sugar", None, None).await;
    assert!(matches!(res, Err(EpicError::NoActiveAccount)));
}

#[tokio::test]
async fn test_adversarial_store_launch_unknown_account_rejected() {
    let (_, _, store) = setup_challenger_store().await;
    let res = store
        .launch_game(Some("nonexistent_account_id"), "Sugar", None, None)
        .await;
    assert!(matches!(res, Err(EpicError::AccountNotFound(_))));
}

#[tokio::test]
async fn test_adversarial_store_launch_game_full_flow_simulated() {
    let (mock, _, store) = setup_challenger_store().await;
    let acc_id = "acc_sim_launch";
    // 32-character hex code
    let code = "0123456789abcdef0123456789abcdef";
    mock.register_auth_code(code, acc_id, "SimGamer").await;

    store.login_with_code(code).await.unwrap();

    let options = LaunchGameOptions {
        account_id: Some(acc_id.to_string()),
        app_name: "SugarGame".to_string(),
        namespace: Some("sandbox_sugar".to_string()),
        locale: Some("en".to_string()),
        additional_args: Some(vec!["-novid".to_string(), "-fullscreen".to_string()]),
        dry_run: true,
    };

    let result = store
        .launch_game_with_options(options)
        .await
        .expect("Simulated store launch should succeed");

    assert_eq!(result.app_name, "SugarGame");
    assert_eq!(result.pid, Some(42424));

    // Raw arguments must contain sensitive exchange token
    assert!(
        result.launch_arguments[1].starts_with("-AUTH_PASSWORD="),
        "Raw launch args must contain exchange code"
    );

    // Display arguments MUST have password masked
    assert_eq!(
        result.display_arguments[1], "-AUTH_PASSWORD=********",
        "Display arguments must have password masked"
    );

    // Verify ordering in both raw and display args
    assert_eq!(result.launch_arguments[0], "-AUTH_LOGIN=unused");
    assert_eq!(result.launch_arguments[2], "-AUTH_TYPE=exchangecode");
    assert_eq!(result.launch_arguments[3], "-epicapp=SugarGame");
    assert_eq!(result.launch_arguments[4], "-epicenv=Prod");
    assert_eq!(result.launch_arguments[5], "-EpicPortal");

    assert_eq!(result.display_arguments[0], "-AUTH_LOGIN=unused");
    assert_eq!(result.display_arguments[2], "-AUTH_TYPE=exchangecode");
    assert_eq!(result.display_arguments[3], "-epicapp=SugarGame");
    assert_eq!(result.display_arguments[4], "-epicenv=Prod");
    assert_eq!(result.display_arguments[5], "-EpicPortal");

    // Check appended custom args
    assert_eq!(result.launch_arguments[10], "-novid");
    assert_eq!(result.launch_arguments[11], "-fullscreen");
}

#[tokio::test]
async fn test_adversarial_rest_api_launch_endpoints_validation() {
    let (mock, db, store) = setup_challenger_store().await;
    let app_state = AppState::new(db, store.clone());
    let app = create_app_with_state(app_state);

    // Test 1: Empty app_name -> 400 Bad Request
    let req1 = Request::builder()
        .method("POST")
        .uri("/api/epic/launch")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"app_name": "   "}"#))
        .unwrap();
    let res1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(res1.status(), StatusCode::BAD_REQUEST);

    // Test 2: Missing account and no active account -> 400 Bad Request
    let req2 = Request::builder()
        .method("POST")
        .uri("/api/epic/launch")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"app_name": "Sugar"}"#))
        .unwrap();
    let res2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(res2.status(), StatusCode::BAD_REQUEST);

    // Test 3: Non-existent account -> 404 Not Found
    let req3 = Request::builder()
        .method("POST")
        .uri("/api/epic/launch")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"account_id": "ghost_acc", "app_name": "Sugar"}"#))
        .unwrap();
    let res3 = app.clone().oneshot(req3).await.unwrap();
    assert_eq!(res3.status(), StatusCode::NOT_FOUND);

    // Test 4: Valid account with simulation environment variable
    let acc_id = "acc_rest_launch";
    // 32-character hex code
    let code = "fedcba9876543210fedcba9876543210";
    mock.register_auth_code(code, acc_id, "RestGamer").await;
    store.login_with_code(code).await.unwrap();

    std::env::set_var("SWCH_EPIC_SIMULATE_LAUNCH", "1");
    let req4 = Request::builder()
        .method("POST")
        .uri("/api/epic/launch-game")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"account_id": "{}", "app_name": "SugarGame"}}"#,
            acc_id
        )))
        .unwrap();
    let res4 = app.clone().oneshot(req4).await.unwrap();
    std::env::remove_var("SWCH_EPIC_SIMULATE_LAUNCH");

    assert_eq!(res4.status(), StatusCode::OK);
    let bytes = res4.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["app_name"], "SugarGame");
    assert_eq!(json["pid"], 42424);

    let display_args_str = json["display_arguments"].to_string();
    assert!(display_args_str.contains("-AUTH_PASSWORD=********"));
}
