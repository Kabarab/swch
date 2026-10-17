use std::fs;
use swch::launchers::steam::{
    downloads::{
        app_activities, apply_activity, counter_rate, log_phase, remaining_seconds, snapshot,
        AppActivity, DownloadSnapshot,
    },
    launch_watch::{Decision, LaunchPolicy},
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
fn unchanged_manifest_is_not_live_download_progress() {
    use swch::launchers::steam::downloads::progress_stale;
    for phase in ["downloading", "installing", "verifying"] {
        assert!(progress_stale(phase, Some(1000), 1016));
        assert!(progress_stale(phase, None, 1016));
        assert!(!progress_stale(phase, Some(1015), 1016));
    }
    for phase in ["ready", "paused", "queued"] {
        assert!(!progress_stale(phase, Some(1000), 2000));
    }
}

fn watch_state(phase: &str) -> DownloadSnapshot {
    DownloadSnapshot {
        update: serde_json::from_value(serde_json::json!({"required":phase!="ready","phase":phase,"downloaded":0,"download_total":100,"staged":0,"stage_total":100})).unwrap(),
        is_installed: true,
        steam_running: true,
        is_running: false,
        last_started_at: None,
        last_update_finished_at: None,
    }
}

#[test]
fn launch_unknown_update_waits_for_disk_work_then_retries_only_once() {
    let mut policy = LaunchPolicy::default();
    let requested_at = 1000;
    assert_eq!(
        policy.advance(0, requested_at, Some(&watch_state("ready")), Some(true)),
        Decision::Wait
    );
    assert_eq!(
        policy.advance(
            2,
            requested_at,
            Some(&watch_state("downloading")),
            Some(true)
        ),
        Decision::Wait
    );
    assert_eq!(
        policy.advance(
            4,
            requested_at,
            Some(&watch_state("installing")),
            Some(true)
        ),
        Decision::Wait
    );
    assert_eq!(
        policy.advance(6, requested_at, Some(&watch_state("verifying")), Some(true)),
        Decision::Wait
    );
    assert_eq!(
        policy.advance(8, requested_at, Some(&watch_state("ready")), Some(true)),
        Decision::Wait
    );
    assert_eq!(
        policy.advance(16, requested_at, Some(&watch_state("ready")), Some(true)),
        Decision::Wait
    );
    assert_eq!(
        policy.advance(18, requested_at, Some(&watch_state("ready")), Some(true)),
        Decision::Retry
    );
    assert_eq!(
        policy.advance(20, requested_at, Some(&watch_state("ready")), Some(true)),
        Decision::Finish
    );
}

#[test]
fn launch_watch_does_not_reopen_a_game_that_steam_already_started_or_user_closed() {
    for running in [false, true] {
        let mut policy = LaunchPolicy::default();
        policy.advance(0, 1000, Some(&watch_state("installing")), Some(true));
        let mut state = watch_state("ready");
        state.is_running = running;
        state.last_started_at = Some(1002);
        assert_eq!(
            policy.advance(20, 1000, Some(&state), Some(true)),
            Decision::Finish
        );
    }
}

#[test]
fn launch_watch_handles_short_updates_missing_data_account_changes_and_pause() {
    let mut policy = LaunchPolicy::default();
    let mut done = watch_state("ready");
    done.last_update_finished_at = Some(1001); // Entire update happened between polls.
    assert_eq!(
        policy.advance(2, 1000, Some(&done), Some(true)),
        Decision::Wait
    );
    assert_eq!(
        policy.advance(12, 1000, Some(&done), Some(true)),
        Decision::Retry
    );
    let mut policy = LaunchPolicy::default();
    assert_eq!(
        policy.advance(0, 1000, Some(&watch_state("installing")), Some(true)),
        Decision::Wait
    );
    assert_eq!(
        policy.advance(10, 1000, Some(&watch_state("ready")), Some(true)),
        Decision::Wait
    );
    assert_eq!(policy.advance(20, 1000, None, Some(true)), Decision::Wait);
    assert_eq!(
        policy.advance(30, 1000, Some(&watch_state("paused")), Some(true)),
        Decision::Wait
    );
    assert_eq!(
        policy.advance(40, 1000, Some(&watch_state("ready")), None),
        Decision::Wait
    );
    assert_eq!(
        policy.advance(50, 1000, Some(&watch_state("ready")), Some(false)),
        Decision::Finish
    );
    let mut no_update = LaunchPolicy::default();
    assert_eq!(
        no_update.advance(120, 1000, Some(&watch_state("ready")), Some(true)),
        Decision::Finish
    );
    let mut queued = LaunchPolicy::default();
    assert_eq!(
        queued.advance(121, 1000, Some(&watch_state("queued")), Some(true)),
        Decision::Wait
    );
    assert_eq!(
        queued.advance(600, 1000, Some(&watch_state("paused")), Some(true)),
        Decision::Wait
    );
    let mut closed_client = LaunchPolicy::default();
    closed_client.advance(0, 1000, Some(&watch_state("installing")), Some(true));
    let mut closed = watch_state("ready");
    closed.steam_running = false;
    assert_eq!(
        closed_client.advance(2, 1000, Some(&closed), Some(true)),
        Decision::Finish
    );
}

#[test]
fn activity_tracks_update_completion_running_and_exit_without_reading_shader_phases() {
    let log = "[2026-10-15 08:36:32] AppID 1422450 App update changed : Running Update,Staging,\n\
        [2026-10-15 08:36:35] AppID 1422450 finished update, 3 mounted depots\n\
        [2026-10-15 08:36:35] AppID 1422450 App update changed : None\n\
        [2026-10-15 08:38:22] AppID 1422450 state changed : Fully Installed,App Running,\n\
        [2026-10-15 08:38:25] AppID 730 Shader update changed : Running Update,Downloading,\n\
        [2026-10-15 08:39:00] AppID 1422450 state changed : Fully Installed,\n";
    let activities = app_activities(log);
    let app = &activities["1422450"];
    assert!(!app.is_running);
    assert!(app.last_started_at.unwrap() > app.last_update_finished_at.unwrap());
    assert_eq!(app.phase.unwrap().0, "ready");
    assert!(activities["730"].phase.is_none());
}

#[test]
fn disk_phase_from_steam_log_survives_a_newer_generic_manifest() {
    let active = AppActivity {
        phase: Some(("installing", 1050)),
        ..Default::default()
    };
    let mut state = watch_state("queued").update;
    apply_activity(&mut state, Some(&active), Some(1051), Some(900), 1060, true);
    assert_eq!(state.phase, "installing");
    for phase in ["ready", "paused"] {
        let mut state = watch_state(phase).update;
        apply_activity(
            &mut state,
            Some(&active),
            Some(1051),
            Some(1051),
            1060,
            true,
        );
        assert_eq!(
            state.phase, phase,
            "An older log must not override a completed/paused manifest"
        );
    }
    let mut state = watch_state("installing").update;
    let stopped = AppActivity {
        phase: Some(("ready", 1060)),
        ..Default::default()
    };
    apply_activity(
        &mut state,
        Some(&stopped),
        Some(1051),
        Some(900),
        1060,
        true,
    );
    assert_eq!(
        state.phase, "installing",
        "Steam's None event isn't proof of a completed installation"
    );
}

#[tokio::test]
async fn newer_launch_intent_replaces_old_request_and_can_be_cancelled() {
    use swch::launchers::steam::launch_watch;
    let temp = tempfile::tempdir().unwrap();
    launch_watch::start(
        SteamPaths::new(temp.path()),
        "730".into(),
        "76561197960265729".into(),
        vec![],
    );
    assert_eq!(launch_watch::status().unwrap().app_id, "730");
    launch_watch::start(
        SteamPaths::new(temp.path()),
        "570".into(),
        "76561197960265730".into(),
        vec![],
    );
    assert_eq!(launch_watch::status().unwrap().app_id, "570");
    launch_watch::cancel(Some("730"));
    assert_eq!(launch_watch::status().unwrap().app_id, "570");
    launch_watch::cancel(Some("570"));
    assert!(launch_watch::status().is_none());
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
