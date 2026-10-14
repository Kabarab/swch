use std::fs;
use swch::launchers::steam::{
    downloads::{counter_rate, log_phase, remaining_seconds, snapshot},
    library::scan,
    SteamPaths,
};

#[test]
fn rates_and_eta_handle_pause_counter_reset_and_missing_samples() {
    assert_eq!(counter_rate(100, 500, 2.0), Some(200.0));
    assert_eq!(remaining_seconds(500, 1100, Some(200.0)), Some(3));
    assert_eq!(counter_rate(500, 100, 2.0), None);
    assert_eq!(counter_rate(100, 200, 0.0), None);
    assert_eq!(counter_rate(100, 200, 60.0), None);
    assert_eq!(remaining_seconds(100, 200, Some(0.0)), None);
    assert_eq!(remaining_seconds(200, 200, Some(10.0)), None);
    assert_eq!(remaining_seconds(100, 200, Some(f64::NAN)), None);
}

#[test]
fn steam_log_distinguishes_network_staging_validation_and_commit() {
    let line = |phase| format!("[2026-10-14 20:48:14] AppID 730 App update changed : {phase}");
    assert_eq!(
        log_phase(&line("Running Update,Downloading,Staging,")),
        Some("downloading")
    );
    assert_eq!(
        log_phase(&line("Running Update,Verifying Staged,")),
        Some("verifying")
    );
    assert_eq!(
        log_phase(&line("Running Update,Committing,")),
        Some("installing")
    );
    assert_eq!(
        log_phase(&line("Running Update,Preallocating,")),
        Some("preparing")
    );
    assert_eq!(
        log_phase(&line("Running Update,Downloading,Stopping,")),
        Some("paused")
    );
    assert_eq!(log_phase(&line("None")), Some("ready"));
    assert_eq!(
        log_phase("AppID 730 Shader update changed : Running Update,Downloading,"),
        None
    );
}

#[test]
fn manifest_flags_do_not_mistake_started_or_corrupt_for_download_or_validation() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("steamapps")).unwrap();
    let path = temp.path().join("steamapps/appmanifest_730.acf");
    for (flags, phase) in [
        (4, "ready"),
        (1030, "queued"),
        (518, "paused"),
        (1049862, "downloading"),
        (2097158, "installing"),
        (4194310, "installing"),
        (131078, "verifying"),
        (524294, "preparing"),
        (132, "required"),
    ] {
        fs::write(&path, format!(r#""AppState" {{ "StateFlags" "{flags}" "BytesToDownload" "1000" "BytesDownloaded" "250" }}"#)).unwrap();
        let data = scan(&SteamPaths::new(temp.path())).unwrap();
        assert_eq!(
            data.games[0].update.as_ref().unwrap().phase,
            phase,
            "flags={flags}"
        );
    }
}

#[test]
fn progress_scan_reads_all_library_roots_without_requiring_loginusers() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Steam");
    let external = temp.path().join("Library");
    fs::create_dir_all(root.join("steamapps")).unwrap();
    fs::create_dir_all(external.join("steamapps")).unwrap();
    let escaped = external.to_string_lossy().replace('\\', "\\\\");
    fs::write(
        root.join("steamapps/libraryfolders.vdf"),
        format!(r#""libraryfolders" {{ "1" {{ "path" "{escaped}" }} }}"#),
    )
    .unwrap();
    fs::write(
        external.join("steamapps/appmanifest_730.acf"),
        r#""AppState" { "StateFlags" "1049862" "BytesToDownload" "1000" "BytesDownloaded" "250" }"#,
    )
    .unwrap();
    let games = snapshot(&SteamPaths::new(root));
    assert_eq!(games["730"].update.downloaded, 250);
    assert_eq!(games["730"].update.download_total, 1000);
    assert_eq!(
        games["730"].update.telemetry.as_ref().unwrap().eta_seconds,
        None
    );
}
