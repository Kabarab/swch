//! Read-only Steam download snapshots. Counters belong to each app; process I/O
//! and the content log's network rate belong to the entire Steam client.
use super::{
    library::{self, UpdateState},
    SteamPaths,
};
use crate::parsers::vdf::VdfDocument;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek, SeekFrom},
    sync::{Mutex, OnceLock},
    time::Instant,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DownloadTelemetry {
    #[serde(default)]
    pub data_stale: bool,
    #[serde(default)]
    pub progress_stale: bool,
    pub download_bps: Option<f64>,
    pub stage_bps: Option<f64>,
    pub eta_seconds: Option<u64>,
    pub steam_download_bps: Option<f64>,
    pub steam_read_bps: Option<f64>,
    pub steam_write_bps: Option<f64>,
}
#[derive(Debug, Clone, Serialize)]
pub struct DownloadSnapshot {
    pub update: UpdateState,
    pub is_installed: bool,
    pub steam_running: bool,
    pub is_running: bool,
    pub last_started_at: Option<i64>,
    pub last_update_finished_at: Option<i64>,
}

#[derive(Debug, Default)]
pub struct AppActivity {
    pub phase: Option<(&'static str, i64)>,
    pub is_running: bool,
    pub last_started_at: Option<i64>,
    pub last_update_finished_at: Option<i64>,
}

pub fn app_activities(log: &str) -> BTreeMap<String, AppActivity> {
    let mut apps: BTreeMap<String, AppActivity> = BTreeMap::new();
    for line in log.lines() {
        let Some((_, rest)) = line.split_once("AppID ") else {
            continue;
        };
        let Some(id) = rest
            .split_whitespace()
            .next()
            .filter(|id| library::valid_app_id(id))
        else {
            continue;
        };
        let Some(stamp) = log_timestamp(line) else {
            continue;
        };
        let app = apps.entry(id.to_owned()).or_default();
        if let Some(phase) = log_phase(line) {
            app.phase = Some((phase, stamp));
        }
        if rest.contains(" state changed :") {
            app.is_running = rest.contains("App Running");
            if app.is_running {
                app.last_started_at = Some(stamp);
            }
        }
        if rest.contains(" finished update,") {
            app.last_update_finished_at = Some(stamp);
        }
    }
    apps
}

pub fn apply_activity(
    update: &mut UpdateState,
    activity: Option<&AppActivity>,
    modified: Option<i64>,
    last_update: Option<i64>,
    now: i64,
    running: bool,
) {
    let Some((phase, stamp)) = activity.and_then(|a| a.phase) else {
        return;
    };
    // Generic started/queued manifests can be rewritten after a more precise
    // log entry. A new file mtime alone does not mean downloading resumed.
    let generic_active = update.required
        && matches!(update.phase.as_str(), "required" | "queued" | "preparing")
        && last_update.is_some_and(|t| stamp >= t)
        && (0..7200).contains(&(now - stamp));
    // "None" is also written when a job fails/stops, so it cannot confirm ready.
    if phase != "ready" && running && (modified.is_some_and(|m| stamp >= m) || generic_active) {
        update.phase = phase.into();
        update.required = true;
    }
}

pub fn counter_rate(previous: u64, current: u64, elapsed: f64) -> Option<f64> {
    (elapsed >= 0.5 && elapsed <= 15.0 && current >= previous)
        .then(|| (current - previous) as f64 / elapsed)
}
pub fn remaining_seconds(done: u64, total: u64, rate: Option<f64>) -> Option<u64> {
    rate.filter(|r| r.is_finite() && *r > 0.0)
        .filter(|_| total > done)
        .map(|r| ((total - done) as f64 / r).ceil() as u64)
}

/// Steam can leave its manifest unchanged throughout an active transfer.
/// Such counters are a checkpoint, not a measurement of current progress.
pub fn progress_stale(phase: &str, modified: Option<i64>, now: i64) -> bool {
    matches!(phase, "downloading" | "installing" | "verifying")
        && !modified.is_some_and(|stamp| (0..=15).contains(&(now - stamp)))
}

#[derive(Default)]
struct Sampler {
    previous: BTreeMap<String, DownloadSnapshot>,
    sampled_at: Option<Instant>,
    root: Option<std::path::PathBuf>,
    processes: sysinfo::System,
}
static SAMPLER: OnceLock<Mutex<Sampler>> = OnceLock::new();

// Read a bounded tail; logs can grow to hundreds of MB. Never touch login data.
fn content_tail(paths: &SteamPaths) -> String {
    let Ok(mut file) = fs::File::open(paths.steam_dir.join("logs/content_log.txt")) else {
        return String::new();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(2 * 1024 * 1024);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    if file.take(2 * 1024 * 1024).read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if start > 0 {
        text.split_once('\n')
            .map(|(_, tail)| tail.to_owned())
            .unwrap_or_default()
    } else {
        text
    }
}

pub fn log_phase(line: &str) -> Option<&'static str> {
    if line.contains("App update changed :") {
        let phase = line.split_once("App update changed :")?.1;
        if phase.contains("Stopping") {
            Some("paused")
        } else if phase.contains("Downloading") {
            Some("downloading")
        } else if phase.contains("Verifying") {
            Some("verifying")
        } else if phase.contains("Staging") || phase.contains("Committing") {
            Some("installing")
        } else if phase.contains("Running Update") {
            Some("preparing")
        } else if phase.trim() == "None" {
            Some("ready")
        } else {
            None
        }
    } else if line.contains("state changed :") {
        if line.contains("Update Paused") {
            Some("paused")
        } else if line.contains("Update Required") && !line.contains("Update Running") {
            Some(if line.contains("Update Queued") {
                "queued"
            } else {
                "required"
            })
        } else {
            None
        }
    } else {
        None
    }
}

fn log_timestamp(line: &str) -> Option<i64> {
    use chrono::TimeZone;
    let stamp =
        chrono::NaiveDateTime::parse_from_str(line.get(1..20)?, "%Y-%m-%d %H:%M:%S").ok()?;
    chrono::Local
        .from_local_datetime(&stamp)
        .single()
        .map(|t| t.timestamp())
}

pub fn snapshot(paths: &SteamPaths) -> BTreeMap<String, DownloadSnapshot> {
    let mut sampler = SAMPLER
        .get_or_init(|| Mutex::new(Sampler::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if sampler.root.as_ref() != Some(&paths.steam_dir) {
        *sampler = Sampler {
            root: Some(paths.steam_dir.clone()),
            ..Default::default()
        };
    }
    let now = Instant::now();
    let elapsed = sampler
        .sampled_at
        .map(|t| now.duration_since(t).as_secs_f64());
    // Multiple UI windows share one sample rather than resetting each other's rates.
    if elapsed.is_some_and(|s| s < 1.0) {
        return sampler.previous.clone();
    }
    let known_processes: std::collections::HashSet<_> = sampler
        .processes
        .processes()
        .iter()
        .map(|(pid, p)| (*pid, p.start_time()))
        .collect();
    sampler
        .processes
        .refresh_processes_specifics(sysinfo::ProcessRefreshKind::new().with_disk_usage());
    let mut reads = 0;
    let mut writes = 0;
    let mut running = false;
    for (pid, process) in sampler.processes.processes() {
        if !matches!(
            process.name().to_ascii_lowercase().as_str(),
            "steam" | "steam.exe" | "steam_osx"
        ) {
            continue;
        }
        running = true;
        let usage = process.disk_usage();
        // A newly observed process has no baseline; its lifetime bytes aren't a rate.
        if known_processes.contains(&(*pid, process.start_time())) {
            reads += usage.read_bytes;
            writes += usage.written_bytes;
        }
    }
    let read_bps = elapsed
        .filter(|s| running && *s >= 0.5 && *s <= 15.0)
        .map(|s| reads as f64 / s);
    let write_bps = elapsed
        .filter(|s| running && *s >= 0.5 && *s <= 15.0)
        .map(|s| writes as f64 / s);
    let log = content_tail(paths);
    let activities = app_activities(&log);
    let mut steam_download_bps = None;
    for line in log.lines() {
        if let Some((_, rate)) = line.split_once("Current download rate: ") {
            steam_download_bps = log_timestamp(line)
                .filter(|stamp| (0..=75).contains(&(chrono::Utc::now().timestamp() - stamp)))
                .and_then(|_| rate.strip_suffix(" Mbps"))
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|n| n.is_finite() && *n >= 0.0)
                .map(|mbps| mbps * 1_000_000.0 / 8.0);
        }
    }
    let mut games = BTreeMap::new();
    for root in library::library_roots(paths) {
        let Ok(entries) = fs::read_dir(root.join("steamapps")) else {
            continue;
        };
        for entry in entries.flatten() {
            let filename = entry.file_name().to_string_lossy().into_owned();
            let Some(id) = filename
                .strip_prefix("appmanifest_")
                .and_then(|s| s.strip_suffix(".acf"))
                .filter(|id| library::valid_app_id(id) && !library::is_ignored_steam_app(id, ""))
            else {
                continue;
            };
            let Ok(doc) = VdfDocument::from_file(entry.path()) else {
                // Steam may be replacing a manifest. Keep its last good sample.
                if let Some(previous) = sampler.previous.get(id) {
                    let mut previous = previous.clone();
                    previous.update.telemetry = Some(DownloadTelemetry {
                        data_stale: true,
                        ..Default::default()
                    });
                    games.insert(id.to_owned(), previous);
                }
                continue;
            };
            let Some(state) = doc.get_obj("AppState") else {
                continue;
            };
            let mut update = UpdateState::from_manifest(state);
            let is_installed = state
                .get_str("StateFlags")
                .and_then(|s| s.parse::<u64>().ok())
                .is_some_and(|f| f & 4 != 0);
            let modified = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| chrono::DateTime::<chrono::Utc>::from(t).timestamp());
            let activity = activities.get(id);
            let last_update = state
                .get_str("LastUpdated")
                .and_then(|s| s.parse::<i64>().ok());
            apply_activity(
                &mut update,
                activity,
                modified,
                last_update,
                chrono::Utc::now().timestamp(),
                running,
            );
            let active = matches!(
                update.phase.as_str(),
                "downloading" | "installing" | "verifying" | "preparing"
            );
            let mut telemetry = DownloadTelemetry::default();
            telemetry.progress_stale = progress_stale(
                &update.phase, modified, chrono::Utc::now().timestamp(),
            );
            if active {
                telemetry.steam_read_bps = read_bps;
                telemetry.steam_write_bps = write_bps;
                if update.phase == "downloading" {
                    telemetry.steam_download_bps = steam_download_bps;
                }
                if let (Some(previous), Some(seconds)) = (sampler.previous.get(id), elapsed) {
                    let prev = &previous.update;
                    if !telemetry.progress_stale
                        && prev.download_total == update.download_total
                        && prev.stage_total == update.stage_total
                        && prev.phase != "ready"
                        && prev.phase != "paused"
                    {
                        telemetry.download_bps =
                            counter_rate(prev.downloaded, update.downloaded, seconds);
                        telemetry.stage_bps = counter_rate(prev.staged, update.staged, seconds);
                        telemetry.eta_seconds = if update.phase == "downloading" {
                            remaining_seconds(
                                update.downloaded,
                                update.download_total,
                                telemetry.download_bps,
                            )
                        } else if update.phase == "installing" {
                            remaining_seconds(
                                update.staged,
                                update.stage_total,
                                telemetry.stage_bps,
                            )
                        } else {
                            None
                        };
                    }
                }
            }
            update.telemetry = Some(telemetry);
            games.insert(
                id.to_owned(),
                DownloadSnapshot {
                    update,
                    is_installed,
                    steam_running: running,
                    is_running: running && activity.is_some_and(|a| a.is_running),
                    last_started_at: activity.and_then(|a| a.last_started_at),
                    last_update_finished_at: activity.and_then(|a| a.last_update_finished_at),
                },
            );
        }
    }
    sampler.sampled_at = Some(now);
    sampler.previous = games.clone();
    games
}
