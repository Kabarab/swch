use std::fs;
use tempfile::tempdir;

use swch::launchers::steam::{SteamError, SteamPaths, SteamSessionManager};
use swch::parsers::vdf::VdfDocument;
use swch::storage::db::Database;
use swch::storage::models::Account;

#[tokio::test]
async fn test_full_hot_swap_lifecycle_with_sentry_and_db() {
    let dir = tempdir().unwrap();
    let steam_dir = dir.path().join("Steam");
    let config_dir = steam_dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();

    let sessions_dir = dir.path().join("swch_sessions");
    fs::create_dir_all(&sessions_dir).unwrap();

    // 1. Create a realistic loginusers.vdf with 3 users
    let loginusers_initial = r#""users"
{
	"76561198000000001"
	{
		"AccountName"		"commander_shepard"
		"PersonaName"		"Shepard"
		"RememberPassword"		"1"
		"MostRecent"		"1"
		"Timestamp"		"1700000000"
		"WantsOfflineMode"		"0"
		"SkipOfflineModeWarning"		"0"
	}
	"76561198000000002"
	{
		"AccountName"		"garrus_vakarian"
		"PersonaName"		"Archangel"
		"RememberPassword"		"1"
		"MostRecent"		"0"
		"Timestamp"		"1690000000"
		"WantsOfflineMode"		"0"
		"SkipOfflineModeWarning"		"0"
	}
	"76561198000000003"
	{
		"AccountName"		"liara_tsoni"
		"PersonaName"		"Shadow Broker"
		"RememberPassword"		"0"
		"MostRecent"		"0"
		"Timestamp"		"1680000000"
	}
}
"#;
    fs::write(config_dir.join("loginusers.vdf"), loginusers_initial).unwrap();

    // 2. Create existing registry.vdf and config.vdf
    let initial_registry = r#""Registry"
{
	"HKCU"
	{
		"Software"
		{
			"Valve"
			{
				"Steam"
				{
					"AutoLoginUser"		"commander_shepard"
					"RememberPassword"		"1"
				}
			}
		}
	}
}
"#;
    fs::write(steam_dir.join("registry.vdf"), initial_registry).unwrap();

    let initial_config = r#""InstallConfigStore"
{
	"Software"
	{
		"Valve"
		{
			"Steam"
			{
				"AutoLoginUser"		"commander_shepard"
			}
		}
	}
}
"#;
    fs::write(config_dir.join("config.vdf"), initial_config).unwrap();

    // 3. Create sentry files for current user (commander_shepard) in Steam root
    let shepard_sentry = steam_dir.join("ssfn1111111111111111111");
    fs::write(&shepard_sentry, b"sentry_token_shepard").unwrap();

    // 4. Pre-stage a backed-up sentry file for garrus in swch sessions
    let garrus_backup_dir = sessions_dir.join("garrus_vakarian");
    fs::create_dir_all(&garrus_backup_dir).unwrap();
    let garrus_sentry_backup = garrus_backup_dir.join("ssfn2222222222222222222");
    fs::write(&garrus_sentry_backup, b"sentry_token_garrus").unwrap();

    // 5. Initialize in-memory SQLite database with accounts
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let acc1 = Account {
        id: "steam_commander_shepard".to_string(),
        launcher: "steam".to_string(),
        username: "commander_shepard".to_string(),
        display_name: Some("Shepard".to_string()),
        email: None,
        is_active: true,
        last_used: Some("2026-09-10T10:00:00Z".to_string()),
    };
    let acc2 = Account {
        id: "steam_garrus_vakarian".to_string(),
        launcher: "steam".to_string(),
        username: "garrus_vakarian".to_string(),
        display_name: Some("Archangel".to_string()),
        email: None,
        is_active: false,
        last_used: Some("2026-09-01T10:00:00Z".to_string()),
    };
    db.upsert_account(&acc1).await.unwrap();
    db.upsert_account(&acc2).await.unwrap();

    // 6. Perform Hot-Swap to garrus_vakarian (using case-insensitive input)
    let paths = SteamPaths::new(&steam_dir);
    let manager = SteamSessionManager::with_paths(paths.clone())
        .with_sessions_dir(&sessions_dir)
        .with_skip_process_termination(true);

    let result = manager
        .switch_account("GaRrUs_VaKaRiAn", Some(&db))
        .await
        .expect("Switch account should succeed");

    assert_eq!(result.account_name, "garrus_vakarian");
    assert_eq!(
        result.previous_account,
        Some("commander_shepard".to_string())
    );
    assert_eq!(result.steam_id, Some("76561198000000002".to_string()));

    // 7. Verify loginusers.vdf AST modifications
    let updated_vdf = VdfDocument::from_file(paths.loginusers_path()).unwrap();
    assert_eq!(
        updated_vdf.get_path_str(&["users", "76561198000000001"], "MostRecent"),
        Some("0")
    );
    assert_eq!(
        updated_vdf.get_path_str(&["users", "76561198000000002"], "MostRecent"),
        Some("1")
    );
    assert_eq!(
        updated_vdf.get_path_str(&["users", "76561198000000002"], "RememberPassword"),
        Some("1")
    );
    assert_eq!(
        updated_vdf.get_path_str(&["users", "76561198000000003"], "MostRecent"),
        Some("0")
    );

    // 8. Verify registry.vdf update
    let updated_reg = VdfDocument::from_file(paths.registry_vdf_path()).unwrap();
    assert_eq!(
        updated_reg.get_path_str(
            &["Registry", "HKCU", "Software", "Valve", "Steam"],
            "AutoLoginUser"
        ),
        Some("garrus_vakarian")
    );
    assert_eq!(
        updated_reg.get_path_str(
            &["Registry", "HKCU", "Software", "Valve", "Steam"],
            "RememberPassword"
        ),
        Some("1")
    );

    // 9. Verify config.vdf update
    let updated_cfg = VdfDocument::from_file(paths.config_vdf_path()).unwrap();
    assert_eq!(
        updated_cfg.get_path_str(
            &["InstallConfigStore", "Software", "Valve", "Steam"],
            "AutoLoginUser"
        ),
        Some("garrus_vakarian")
    );

    // 10. Verify sentry file restoration
    let restored_garrus_sentry = steam_dir.join("ssfn2222222222222222222");
    assert!(restored_garrus_sentry.exists());
    assert_eq!(
        fs::read(restored_garrus_sentry).unwrap(),
        b"sentry_token_garrus"
    );

    // 11. Verify previous user (commander_shepard) session was backed up into swch storage
    let shepard_backup_dir = sessions_dir.join("commander_shepard");
    assert!(shepard_backup_dir.exists());
    assert!(shepard_backup_dir.join("ssfn1111111111111111111").exists());
    assert!(shepard_backup_dir.join("loginusers.vdf").exists());
    assert!(shepard_backup_dir.join("session_manifest.json").exists());

    // Verify sentry isolation: garrus backup must NOT contain shepard's sentry token
    assert!(
        !garrus_backup_dir.join("ssfn1111111111111111111").exists(),
        "Target account backup must not contain previous account sentry token"
    );

    // 12. Verify SQLite database sync
    let accounts = db.get_accounts().await.unwrap();
    let shepard_db = accounts
        .iter()
        .find(|a| a.username == "commander_shepard")
        .unwrap();
    let garrus_db = accounts
        .iter()
        .find(|a| a.username == "garrus_vakarian")
        .unwrap();

    assert!(!shepard_db.is_active);
    assert!(garrus_db.is_active);
    assert!(garrus_db.last_used.is_some());
    // Verify no duplicate account entries exist in database
    assert_eq!(accounts.len(), 2);
}

#[tokio::test]
async fn test_hot_swap_account_not_in_loginusers_returns_error() {
    let dir = tempdir().unwrap();
    let steam_dir = dir.path().join("Steam");
    let config_dir = steam_dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();

    let loginusers = r#""users"
{
	"76561198000000001"
	{
		"AccountName"		"known_user"
		"PersonaName"		"Known"
		"RememberPassword"		"1"
		"MostRecent"		"1"
	}
}
"#;
    fs::write(config_dir.join("loginusers.vdf"), loginusers).unwrap();

    let paths = SteamPaths::new(&steam_dir);
    let manager = SteamSessionManager::with_paths(paths).with_skip_process_termination(true);

    let err = manager
        .switch_account("ghost_user", None)
        .await
        .unwrap_err();
    match err {
        SteamError::AccountNotFound(name) => assert_eq!(name, "ghost_user"),
        other => panic!("Unexpected error: {:?}", other),
    }
}

#[tokio::test]
async fn test_hot_swap_deduplicates_db_on_repeated_switches() {
    let dir = tempdir().unwrap();
    let steam_dir = dir.path().join("Steam");
    let config_dir = steam_dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let sessions_dir = dir.path().join("swch_sessions");

    let loginusers = r#""users"
{
	"76561198000000001"
	{
		"AccountName"		"player_one"
		"PersonaName"		"One"
		"RememberPassword"		"1"
		"MostRecent"		"1"
	}
	"76561198000000002"
	{
		"AccountName"		"player_two"
		"PersonaName"		"Two"
		"RememberPassword"		"1"
		"MostRecent"		"0"
	}
}
"#;
    fs::write(config_dir.join("loginusers.vdf"), loginusers).unwrap();

    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();

    let paths = SteamPaths::new(&steam_dir);
    let manager = SteamSessionManager::with_paths(paths)
        .with_sessions_dir(&sessions_dir)
        .with_skip_process_termination(true);

    // Switch to player_two twice
    manager
        .switch_account("player_two", Some(&db))
        .await
        .unwrap();
    manager
        .switch_account("player_two", Some(&db))
        .await
        .unwrap();

    let accounts = db.get_accounts().await.unwrap();
    let p2_count = accounts
        .iter()
        .filter(|a| a.username == "player_two")
        .count();
    assert_eq!(
        p2_count, 1,
        "There should never be duplicate account rows in database"
    );
}

#[tokio::test]
async fn test_detect_active_steam_user_autologin_and_library_scan() {
    let dir = tempdir().unwrap();
    let steam_dir = dir.path().join("Steam");
    let config_dir = steam_dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();

    // loginusers.vdf without MostRecent = "1", but with AutoLogin = "1" (macOS modern Steam pattern)
    let loginusers = r#""users"
{
	"76561199123282553"
	{
		"AccountName"		"cepud33"
		"PersonaName"		"nazario2015"
		"RememberPassword"		"1"
		"AutoLogin"		"1"
		"Timestamp"		"1789825065"
	}
	"76561198809112858"
	{
		"AccountName"		"itzm3g4"
		"PersonaName"		"M3G47RON"
		"RememberPassword"		"1"
		"AutoLogin"		"0"
		"Timestamp"		"1789737980"
	}
}
"#;
    fs::write(config_dir.join("loginusers.vdf"), loginusers).unwrap();

    let paths = SteamPaths::new(&steam_dir);
    let doc = VdfDocument::from_file(paths.loginusers_path()).unwrap();
    let users = doc.get_obj("users").unwrap();

    let detected = swch::launchers::steam::detect_active_steam_user(&paths, users);
    assert_eq!(detected, Some("76561199123282553".to_string()));

    // Verify library scan marks cepud33 as active
    let lib = swch::launchers::steam::library::scan(&paths).unwrap();
    let active_acc = lib.accounts.iter().find(|a| a.is_active);
    assert!(active_acc.is_some());
    assert_eq!(active_acc.unwrap().id, "76561199123282553");
    assert_eq!(active_acc.unwrap().username, "cepud33");
}

#[test]
fn test_detect_active_steam_user_registry_and_timestamp_fallbacks() {
    let dir = tempdir().unwrap();
    let steam_dir = dir.path().join("Steam");
    let config_dir = steam_dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();

    let loginusers = r#""users"
{
	"76561198000000001"
	{
		"AccountName"		"alpha"
		"PersonaName"		"AlphaUser"
		"Timestamp"		"1000"
	}
	"76561198000000002"
	{
		"AccountName"		"beta"
		"PersonaName"		"BetaUser"
		"Timestamp"		"2000"
	}
}
"#;
    fs::write(config_dir.join("loginusers.vdf"), loginusers).unwrap();
    let paths = SteamPaths::new(&steam_dir);

    // Case 1: Registry defines AutoLoginUser "alpha"
    let registry = r#""Registry"
{
	"HKCU"
	{
		"Software"
		{
			"Valve"
			{
				"Steam"
				{
					"AutoLoginUser"		"alpha"
				}
			}
		}
	}
}
"#;
    fs::write(steam_dir.join("registry.vdf"), registry).unwrap();
    let doc = VdfDocument::from_file(paths.loginusers_path()).unwrap();
    let users = doc.get_obj("users").unwrap();
    assert_eq!(
        swch::launchers::steam::detect_active_steam_user(&paths, users),
        Some("76561198000000001".to_string())
    );

    // Case 2: Without registry AutoLoginUser, falls back to highest timestamp ("beta" has 2000 > 1000)
    fs::remove_file(steam_dir.join("registry.vdf")).unwrap();
    assert_eq!(
        swch::launchers::steam::detect_active_steam_user(&paths, users),
        Some("76561198000000002".to_string())
    );
}

