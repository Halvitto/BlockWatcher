use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use blockwatcher_core::{AgentActivity, parse_ccusage_daily};
use chrono::{DateTime, Local, Utc};
use tauri::{AppHandle, Manager};
use tauri_plugin_shell::{ShellExt, process::CommandEvent};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const PANEL_MAX_AGE: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub(crate) struct ActivitySnapshot {
    pub(crate) refreshed_at: Instant,
    pub(crate) observed_at: DateTime<Utc>,
    pub(crate) activities: BTreeMap<String, AgentActivity>,
}

pub(crate) type SharedActivity = Arc<Mutex<Option<ActivitySnapshot>>>;

pub(crate) fn needs_refresh(activity: &SharedActivity, max_age: Duration) -> bool {
    activity.lock().map_or(true, |snapshot| {
        snapshot
            .as_ref()
            .is_none_or(|snapshot| snapshot.refreshed_at.elapsed() > max_age)
    })
}

pub(crate) fn spawn_activity_loop(
    app: AppHandle,
    current: SharedActivity,
    state_wake: mpsc::Sender<()>,
) -> mpsc::Sender<()> {
    let (trigger, requests) = mpsc::channel();
    thread::spawn(move || {
        let config = match empty_config_path(&app) {
            Ok(path) => path,
            Err(error) => {
                eprintln!("BlockWatcher could not prepare ccusage: {error}");
                return;
            }
        };

        loop {
            let day = Local::now().date_naive().to_string();
            match apply_activity_result(
                &current,
                read_activity(&app, &config, &day, COMMAND_TIMEOUT),
                Utc::now(),
            ) {
                Ok(()) => {
                    let _ = state_wake.send(());
                }
                Err(error) => {
                    eprintln!("BlockWatcher kept the last ccusage snapshot: {error}");
                }
            }

            match requests.recv_timeout(REFRESH_INTERVAL) {
                Ok(()) => while requests.try_recv().is_ok() {},
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    trigger
}

fn apply_activity_result(
    current: &SharedActivity,
    result: Result<Vec<AgentActivity>, String>,
    observed_at: DateTime<Utc>,
) -> Result<(), String> {
    let activities = result?
        .into_iter()
        .map(|activity| (activity.id.clone(), activity))
        .collect();
    let mut snapshot = current
        .lock()
        .map_err(|_| "activity snapshot lock poisoned".to_string())?;
    *snapshot = Some(ActivitySnapshot {
        refreshed_at: Instant::now(),
        observed_at,
        activities,
    });
    Ok(())
}

fn empty_config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_cache_dir()
        .map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join("ccusage-empty.json");
    std::fs::write(&path, "{}\n").map_err(|error| error.to_string())?;
    Ok(path)
}

fn read_activity(
    app: &AppHandle,
    config: &Path,
    day: &str,
    timeout: Duration,
) -> Result<Vec<AgentActivity>, String> {
    let arguments = [
        "daily".to_string(),
        "--json".to_string(),
        "--by-agent".to_string(),
        "--offline".to_string(),
        "--no-cost".to_string(),
        "--since".to_string(),
        day.to_string(),
        "--until".to_string(),
        day.to_string(),
        "--config".to_string(),
        config.to_string_lossy().into_owned(),
    ];
    let (mut events, child) = app
        .shell()
        .sidecar("ccusage")
        .map_err(|error| error.to_string())?
        .args(arguments)
        .spawn()
        .map_err(|error| error.to_string())?;
    let (result_tx, result_rx) = mpsc::sync_channel(1);

    thread::spawn(move || {
        let result = tauri::async_runtime::block_on(async move {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut exit_code = None;

            while let Some(event) = events.recv().await {
                match event {
                    CommandEvent::Stdout(mut line) => {
                        stdout.append(&mut line);
                        stdout.push(b'\n');
                    }
                    CommandEvent::Stderr(mut line) => {
                        stderr.append(&mut line);
                        stderr.push(b'\n');
                    }
                    CommandEvent::Error(error) => return Err(error),
                    CommandEvent::Terminated(payload) => {
                        exit_code = payload.code;
                        break;
                    }
                    _ => {}
                }
            }

            if exit_code != Some(0) {
                let message = String::from_utf8_lossy(&stderr);
                return Err(format!(
                    "ccusage exited with {:?}: {}",
                    exit_code,
                    message.trim().chars().take(240).collect::<String>()
                ));
            }
            String::from_utf8(stdout).map_err(|error| error.to_string())
        });
        let _ = result_tx.send(result);
    });

    let stdout = receive_command_result(result_rx, timeout, || {
        let _ = child.kill();
    })?;
    parse_ccusage_daily(&stdout, day).map_err(|error| format!("invalid JSON: {error}"))
}

fn receive_command_result(
    receiver: mpsc::Receiver<Result<String, String>>,
    timeout: Duration,
    abort: impl FnOnce(),
) -> Result<String, String> {
    match receiver.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            abort();
            Err(format!("ccusage timed out after {}s", timeout.as_secs()))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            abort();
            Err("ccusage result channel closed".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn stale_or_missing_activity_requests_a_refresh() {
        let activity = SharedActivity::default();
        assert!(needs_refresh(&activity, PANEL_MAX_AGE));

        *activity.lock().unwrap() = Some(ActivitySnapshot {
            refreshed_at: Instant::now(),
            observed_at: Utc::now(),
            activities: BTreeMap::new(),
        });
        assert!(!needs_refresh(&activity, PANEL_MAX_AGE));
    }

    #[test]
    fn timeout_aborts_the_sidecar() {
        let (_sender, receiver) = mpsc::channel();
        let aborted = AtomicBool::new(false);
        let result = receive_command_result(receiver, Duration::from_millis(1), || {
            aborted.store(true, Ordering::Relaxed);
        });

        assert_eq!(result.unwrap_err(), "ccusage timed out after 0s");
        assert!(aborted.load(Ordering::Relaxed));
    }

    #[test]
    fn failed_reads_preserve_the_last_valid_snapshot() {
        let activity = SharedActivity::default();
        let valid = AgentActivity {
            id: "hermes".into(),
            total_tokens: 42,
            tokens: Default::default(),
            models: Vec::new(),
        };
        apply_activity_result(&activity, Ok(vec![valid]), Utc::now()).unwrap();
        let before = activity
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .activities
            .clone();

        assert!(apply_activity_result(&activity, Err("invalid".into()), Utc::now()).is_err());
        assert_eq!(
            activity.lock().unwrap().as_ref().unwrap().activities,
            before
        );
    }
}
