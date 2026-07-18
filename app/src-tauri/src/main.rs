#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod providers;
mod sidecar;

use std::env;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

use blockwatcher_core::{
    APP_NAME, AgentActivity, ClaudeUsage, CodexLogIndex, CodexLogSource, CodexUsage,
    DEFAULT_SESSION_HOURS, LogIndex, LogSource, MacLogSource, SessionBlock, UsageEntry, burn_rate,
    eta_to_limit, identify_blocks, max_block_tokens, parse_claude_usage_history, projection,
};
use chrono::{DateTime, Utc};
use notify::{RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, Rect, State, WindowEvent,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
};

const STATE_EVENT: &str = "usage-state";
const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const APP_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const PANEL_LABEL: &str = "panel";
const PANEL_MARGIN: f64 = 8.0;
type SharedUsage = Arc<Mutex<Option<UsageState>>>;

#[tauri::command]
fn current_usage(current: State<'_, SharedUsage>) -> Option<UsageState> {
    current.lock().ok().and_then(|state| state.clone())
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Installation {
    cli: bool,
    app: bool,
    data: bool,
    running: bool,
}

impl Installation {
    fn detected(self) -> bool {
        self.cli || self.app || self.data || self.running
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RateWindowState {
    used_percent: Option<f64>,
    window_minutes: u64,
    remaining_minutes: Option<i64>,
    reset_at: Option<String>,
    estimated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelUsageState {
    name: String,
    tokens: u64,
    input: u64,
    output: u64,
    cache_creation: u64,
    cache_read: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderState {
    id: String,
    name: String,
    icon: String,
    installed: bool,
    running: bool,
    active: bool,
    summary: String,
    windows: Vec<RateWindowState>,
    tokens: Option<u64>,
    input: Option<u64>,
    output: Option<u64>,
    cache_creation: Option<u64>,
    cache_read: Option<u64>,
    estimated_limit: Option<u64>,
    models: Vec<ModelUsageState>,
    burn_rate: Option<f64>,
    projected_tokens: Option<u64>,
    eta_at: Option<String>,
    malformed_lines: u64,
    updated_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct UsageState {
    title: String,
    providers: Vec<ProviderState>,
}

struct ProviderSnapshot {
    state: ProviderState,
    observed_at: Option<DateTime<Utc>>,
}

fn model_display_name(id: &str) -> String {
    let Some(rest) = id.strip_prefix("claude-") else {
        return id.into();
    };
    let mut parts = rest.split('-');
    let Some(family) = parts
        .next()
        .filter(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_alphabetic()))
    else {
        return id.into();
    };
    let mut version = parts.collect::<Vec<_>>();
    if version
        .last()
        .is_some_and(|part| part.len() == 8 && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        version.pop();
    }
    if version
        .iter()
        .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return id.into();
    }

    let mut label = family.to_ascii_lowercase();
    label[..1].make_ascii_uppercase();
    if !version.is_empty() {
        label.push(' ');
        label.push_str(&version.join("."));
    }
    label
}

fn model_usage(block: &SessionBlock) -> Vec<ModelUsageState> {
    let mut models = block
        .per_model
        .iter()
        .map(|(name, tokens)| ModelUsageState {
            name: model_display_name(name),
            tokens: tokens.total(),
            input: tokens.input,
            output: tokens.output,
            cache_creation: tokens.cache_creation,
            cache_read: tokens.cache_read,
        })
        .collect::<Vec<_>>();
    models.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.name.cmp(&b.name)));
    models
}

fn activity_models(activity: &AgentActivity) -> Vec<ModelUsageState> {
    let mut models = activity
        .models
        .iter()
        .map(|model| ModelUsageState {
            name: model_display_name(&model.name),
            tokens: model.tokens.total(),
            input: model.tokens.input,
            output: model.tokens.output,
            cache_creation: model.tokens.cache_creation,
            cache_read: model.tokens.cache_read,
        })
        .collect::<Vec<_>>();
    models.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.name.cmp(&b.name)));
    models
}

fn claude_state(
    installation: Installation,
    plan_usage: Option<&ClaudeUsage>,
    entries: &[UsageEntry],
    malformed_lines: u64,
    now: DateTime<Utc>,
) -> ProviderSnapshot {
    let blocks = identify_blocks(entries.to_vec(), DEFAULT_SESSION_HOURS, now);
    let estimated_limit = max_block_tokens(&blocks);
    let active_block = blocks.iter().rev().find(|block| block.is_active);

    if let Some(usage) = plan_usage {
        let windows = usage
            .windows
            .iter()
            .map(|window| RateWindowState {
                used_percent: Some(window.used_percent),
                window_minutes: window.window_minutes,
                remaining_minutes: None,
                reset_at: None,
                estimated: false,
            })
            .collect::<Vec<_>>();
        let summary = windows
            .first()
            .and_then(|window| window.used_percent)
            .map_or_else(|| "Idle".into(), |percent| format!("{percent:.0}%"));
        return ProviderSnapshot {
            state: ProviderState {
                id: "claude".into(),
                name: "Claude".into(),
                icon: "claude".into(),
                installed: true,
                running: installation.running,
                active: true,
                summary,
                windows,
                tokens: None,
                input: None,
                output: None,
                cache_creation: None,
                cache_read: None,
                estimated_limit: None,
                models: active_block.map_or_else(Vec::new, model_usage),
                burn_rate: None,
                projected_tokens: None,
                eta_at: None,
                malformed_lines,
                updated_at: Some(usage.observed_at.to_rfc3339()),
            },
            observed_at: Some(usage.observed_at),
        };
    }

    let Some(block) = active_block else {
        let observed_at = blocks
            .iter()
            .rev()
            .find(|block| !block.is_gap)
            .and_then(|block| block.actual_end);
        return ProviderSnapshot {
            state: ProviderState {
                id: "claude".into(),
                name: "Claude".into(),
                icon: "claude".into(),
                installed: installation.detected(),
                running: installation.running,
                active: false,
                summary: if installation.detected() {
                    "Idle".into()
                } else {
                    "Not detected".into()
                },
                windows: Vec::new(),
                tokens: None,
                input: None,
                output: None,
                cache_creation: None,
                cache_read: None,
                estimated_limit: (estimated_limit > 0).then_some(estimated_limit),
                models: Vec::new(),
                burn_rate: None,
                projected_tokens: None,
                eta_at: None,
                malformed_lines,
                updated_at: observed_at.map(|timestamp| timestamp.to_rfc3339()),
            },
            observed_at,
        };
    };

    let remaining_minutes = block.remaining(now).num_minutes();
    let used_percent =
        (estimated_limit > 0).then(|| block.tokens.total() as f64 / estimated_limit as f64 * 100.0);
    let mut summary = format_remaining(remaining_minutes);
    if let Some(percent) = used_percent {
        summary.push_str(&format!(" · {percent:.0}%"));
    }
    let observed_at = block.actual_end;
    ProviderSnapshot {
        state: ProviderState {
            id: "claude".into(),
            name: "Claude".into(),
            icon: "claude".into(),
            installed: true,
            running: installation.running,
            active: true,
            summary,
            windows: vec![RateWindowState {
                used_percent,
                window_minutes: 300,
                remaining_minutes: Some(remaining_minutes),
                reset_at: Some(block.end.to_rfc3339()),
                estimated: true,
            }],
            tokens: Some(block.tokens.total()),
            input: Some(block.tokens.input),
            output: Some(block.tokens.output),
            cache_creation: Some(block.tokens.cache_creation),
            cache_read: Some(block.tokens.cache_read),
            estimated_limit: (estimated_limit > 0).then_some(estimated_limit),
            models: model_usage(block),
            burn_rate: burn_rate(block).map(|rate| rate.tokens_per_minute),
            projected_tokens: projection(block, now).map(|projection| projection.total_tokens),
            eta_at: eta_to_limit(block, estimated_limit, now)
                .map(|timestamp| timestamp.to_rfc3339()),
            malformed_lines,
            updated_at: observed_at.map(|timestamp| timestamp.to_rfc3339()),
        },
        observed_at,
    }
}

fn codex_state(
    installation: Installation,
    usage: Option<&CodexUsage>,
    activity: Option<&AgentActivity>,
    activity_observed_at: Option<DateTime<Utc>>,
    malformed_lines: u64,
    now: DateTime<Utc>,
) -> ProviderSnapshot {
    let windows = usage
        .into_iter()
        .flat_map(|usage| &usage.windows)
        .map(|window| RateWindowState {
            used_percent: Some(window.used_percent),
            window_minutes: window.window_minutes,
            remaining_minutes: Some((window.resets_at - now).num_minutes().max(0)),
            reset_at: Some(window.resets_at.to_rfc3339()),
            estimated: false,
        })
        .collect::<Vec<_>>();
    let active_window = windows
        .iter()
        .find(|window| window.remaining_minutes.is_some_and(|minutes| minutes > 0));
    let active = active_window.is_some() || activity.is_some();
    let summary = match active_window {
        Some(window) => format!(
            "{:.0}% · {}",
            window.used_percent.unwrap_or_default(),
            format_remaining(window.remaining_minutes.unwrap_or_default())
        ),
        None if installation.detected() => "Idle".into(),
        None => "Not detected".into(),
    };
    let observed_at = usage
        .map(|usage| usage.observed_at)
        .into_iter()
        .chain(activity_observed_at)
        .max();

    ProviderSnapshot {
        state: ProviderState {
            id: "codex".into(),
            name: "Codex".into(),
            icon: "codex".into(),
            installed: installation.detected(),
            running: installation.running,
            active,
            summary,
            windows,
            tokens: activity.map(|activity| activity.total_tokens),
            input: activity.map(|activity| activity.tokens.input),
            output: activity.map(|activity| activity.tokens.output),
            cache_creation: activity.map(|activity| activity.tokens.cache_creation),
            cache_read: activity.map(|activity| activity.tokens.cache_read),
            estimated_limit: None,
            models: activity.map_or_else(Vec::new, activity_models),
            burn_rate: None,
            projected_tokens: None,
            eta_at: None,
            malformed_lines,
            updated_at: observed_at.map(|timestamp| timestamp.to_rfc3339()),
        },
        observed_at,
    }
}

fn generic_state(
    definition: &providers::ProviderDefinition,
    installation: Installation,
    activity: Option<&AgentActivity>,
    observed_at: Option<DateTime<Utc>>,
) -> ProviderSnapshot {
    let summary = if activity.is_some() {
        "Today".into()
    } else if installation.running {
        "Open".into()
    } else if installation.detected() {
        "Installed".into()
    } else {
        "Not detected".into()
    };

    ProviderSnapshot {
        state: ProviderState {
            id: definition.id.into(),
            name: definition.name.into(),
            icon: definition.icon.into(),
            installed: installation.detected(),
            running: installation.running,
            active: installation.running || activity.is_some(),
            summary,
            windows: Vec::new(),
            tokens: activity.map(|activity| activity.total_tokens),
            input: activity.map(|activity| activity.tokens.input),
            output: activity.map(|activity| activity.tokens.output),
            cache_creation: activity.map(|activity| activity.tokens.cache_creation),
            cache_read: activity.map(|activity| activity.tokens.cache_read),
            estimated_limit: None,
            models: activity.map_or_else(Vec::new, activity_models),
            burn_rate: None,
            projected_tokens: None,
            eta_at: None,
            malformed_lines: 0,
            updated_at: activity
                .and(observed_at)
                .map(|timestamp| timestamp.to_rfc3339()),
        },
        observed_at: activity.and(observed_at),
    }
}

fn provider_order(provider: &ProviderState) -> (u8, usize, String) {
    match provider.id.as_str() {
        "claude" => (0, 0, String::new()),
        "codex" => (0, 1, String::new()),
        "chatgpt" => (0, 2, String::new()),
        _ if provider.running => (1, 0, provider.name.to_ascii_lowercase()),
        _ => (2, 0, provider.name.to_ascii_lowercase()),
    }
}

fn usage_state(mut snapshots: Vec<ProviderSnapshot>) -> UsageState {
    snapshots.retain(|provider| provider.state.installed);
    snapshots.sort_by_key(|provider| provider_order(&provider.state));
    let title = snapshots
        .iter()
        .filter(|provider| provider.state.running || provider.state.active)
        .max_by_key(|provider| (provider.state.running, provider.observed_at))
        .map(|provider| provider_menu_text(&provider.state))
        .unwrap_or_else(|| "AI usage".into());

    UsageState {
        title,
        providers: snapshots
            .into_iter()
            .map(|provider| provider.state)
            .collect(),
    }
}

fn format_remaining(minutes: i64) -> String {
    let minutes = minutes.max(0);
    if minutes >= 24 * 60 {
        format!("{}d {}h", minutes / (24 * 60), minutes % (24 * 60) / 60)
    } else {
        format!("{}:{:02}", minutes / 60, minutes % 60)
    }
}

fn provider_menu_text(provider: &ProviderState) -> String {
    provider
        .windows
        .first()
        .and_then(|window| window.used_percent)
        .map_or_else(
            || {
                provider.tokens.map_or_else(
                    || provider.name.clone(),
                    |tokens| format!("{} · {}", provider.name, compact_tokens(tokens)),
                )
            },
            |percent| format!("{} · {percent:.0}%", provider.name),
        )
}

fn compact_tokens(tokens: u64) -> String {
    const UNITS: &[(u64, &str)] = &[(1_000_000_000, "B"), (1_000_000, "M"), (1_000, "K")];
    UNITS
        .iter()
        .find(|(threshold, _)| tokens >= *threshold)
        .map_or_else(
            || tokens.to_string(),
            |(threshold, suffix)| {
                let value = tokens as f64 / *threshold as f64;
                if value >= 10.0 {
                    format!("{value:.0}{suffix}")
                } else {
                    format!("{value:.1}{suffix}")
                }
            },
        )
}

fn claude_plan_history_path() -> Option<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library/Application Support/Claude/plan-usage-history.json"))
        .filter(|path| path.is_file())
}

fn clamp_panel_coordinate(value: f64, origin: i32, available: u32, panel: u32) -> i32 {
    let minimum = f64::from(origin) + PANEL_MARGIN;
    let maximum =
        (f64::from(origin) + f64::from(available) - f64::from(panel) - PANEL_MARGIN).max(minimum);
    value.clamp(minimum, maximum).round() as i32
}

fn show_panel(
    app: &AppHandle,
    rect: Rect,
    current: &SharedUsage,
    state_wake: &mpsc::Sender<()>,
    activity: &sidecar::SharedActivity,
    activity_wake: &mpsc::Sender<()>,
) {
    let _ = state_wake.send(());
    if sidecar::needs_refresh(activity, sidecar::PANEL_MAX_AGE) {
        let _ = activity_wake.send(());
    }

    let Some(panel) = app.get_webview_window(PANEL_LABEL) else {
        return;
    };

    let initial_scale = panel.scale_factor().unwrap_or(1.0);
    let initial_anchor = rect.position.to_physical::<f64>(initial_scale);
    let initial_size = rect.size.to_physical::<f64>(initial_scale);
    let monitor = panel
        .cursor_position()
        .ok()
        .and_then(|cursor| panel.monitor_from_point(cursor.x, cursor.y).ok().flatten())
        .or_else(|| {
            panel
                .monitor_from_point(
                    initial_anchor.x + initial_size.width / 2.0,
                    initial_anchor.y + initial_size.height / 2.0,
                )
                .ok()
                .flatten()
        })
        .or_else(|| panel.primary_monitor().ok().flatten());
    let scale = monitor
        .as_ref()
        .map_or(initial_scale, tauri::window::Monitor::scale_factor);
    let anchor = rect.position.to_physical::<f64>(scale);
    let anchor_size = rect.size.to_physical::<f64>(scale);
    if let Ok(panel_size) = panel.outer_size() {
        let desired_x = anchor.x + anchor_size.width - f64::from(panel_size.width);
        let desired_y = anchor.y + anchor_size.height + 6.0;
        let position = monitor.map_or_else(
            || PhysicalPosition::new(desired_x.round() as i32, desired_y.round() as i32),
            |monitor| {
                let work_area = monitor.work_area();
                PhysicalPosition::new(
                    clamp_panel_coordinate(
                        desired_x,
                        work_area.position.x,
                        work_area.size.width,
                        panel_size.width,
                    ),
                    clamp_panel_coordinate(
                        desired_y,
                        work_area.position.y,
                        work_area.size.height,
                        panel_size.height,
                    ),
                )
            },
        );
        let _ = panel.set_position(position);
    }
    let _ = panel.show();
    let _ = panel.set_focus();

    if let Ok(state) = current.lock()
        && let Some(state) = state.as_ref()
    {
        let _ = app.emit_to(PANEL_LABEL, STATE_EVENT, state.clone());
    }
}

fn update_tray_menu(app: &AppHandle, tray: &TrayIcon, state: &UsageState) -> tauri::Result<()> {
    let menu = Menu::new(app)?;
    if state.providers.is_empty() {
        let empty = MenuItem::with_id(
            app,
            "no-providers",
            "No clients detected",
            false,
            None::<&str>,
        )?;
        menu.append(&empty)?;
    } else {
        for (index, provider) in state.providers.iter().enumerate() {
            let item = MenuItem::with_id(
                app,
                format!("provider-status-{index}"),
                provider_menu_text(provider),
                false,
                None::<&str>,
            )?;
            menu.append(&item)?;
        }
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?)?;
    tray.set_menu(Some(menu))
}

fn spawn_state_loop(
    app: AppHandle,
    tray: TrayIcon,
    current: SharedUsage,
    state_wake: mpsc::Receiver<()>,
    watcher_wake: mpsc::Sender<()>,
    activity: sidecar::SharedActivity,
) {
    thread::spawn(move || {
        let claude_source = MacLogSource::discover();
        let claude_plan_path = claude_plan_history_path();
        let codex_source = CodexLogSource::discover();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event.is_ok() {
                    let _ = watcher_wake.send(());
                }
            })
            .ok();

        if let Some(watcher) = watcher.as_mut() {
            for root in claude_source.roots() {
                let _ = watcher.watch(&root, RecursiveMode::Recursive);
            }
            for root in codex_source.roots() {
                let _ = watcher.watch(&root, RecursiveMode::Recursive);
            }
            if let Some(parent) = claude_plan_path.as_deref().and_then(Path::parent) {
                let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
            }
        }

        let mut claude_index = LogIndex::new();
        let mut codex_index = CodexLogIndex::new();
        let mut apps = providers::AppInventory::discover();
        let mut apps_refreshed_at = Instant::now();
        loop {
            if apps_refreshed_at.elapsed() >= APP_REFRESH_INTERVAL {
                apps = providers::AppInventory::discover();
                apps_refreshed_at = Instant::now();
            }
            let processes = providers::ProcessInventory::discover();
            let claude_source = MacLogSource::discover();
            let claude_plan_path = claude_plan_history_path();
            let claude_plan_usage = claude_plan_path
                .as_deref()
                .and_then(|path| std::fs::read_to_string(path).ok())
                .as_deref()
                .and_then(parse_claude_usage_history);
            let codex_source = CodexLogSource::discover();
            claude_index.refresh(claude_source.log_files());
            codex_index.refresh(codex_source.log_files());

            let now = Utc::now();
            let activity = activity.lock().ok().and_then(|snapshot| snapshot.clone());
            let mut snapshots = Vec::with_capacity(providers::PROVIDERS.len());
            for definition in providers::PROVIDERS {
                let agent_activity = definition
                    .ccusage_id
                    .and_then(|id| activity.as_ref()?.activities.get(id));
                let integrated_codex_open = definition.id == "codex"
                    && processes.app_executable_is_open("ChatGPT.app", "ChatGPT");
                let native_data = match definition.id {
                    "claude" => claude_plan_path.is_some() || !claude_source.roots().is_empty(),
                    "codex" => !codex_source.roots().is_empty(),
                    _ => false,
                };
                let installation = Installation {
                    cli: providers::command_installed(definition),
                    app: apps.contains(definition),
                    data: native_data || agent_activity.is_some(),
                    running: processes.contains(definition) || integrated_codex_open,
                };

                snapshots.push(match definition.id {
                    "claude" => claude_state(
                        installation,
                        claude_plan_usage.as_ref(),
                        claude_index.entries(),
                        claude_index.malformed_lines(),
                        now,
                    ),
                    "codex" => codex_state(
                        installation,
                        codex_index.latest(),
                        agent_activity,
                        activity.as_ref().map(|snapshot| snapshot.observed_at),
                        codex_index.malformed_lines(),
                        now,
                    ),
                    _ => generic_state(
                        definition,
                        installation,
                        agent_activity,
                        activity.as_ref().map(|snapshot| snapshot.observed_at),
                    ),
                });
            }
            let state = usage_state(snapshots);

            let _ = tray.set_title(Some(&state.title));
            let _ = update_tray_menu(&app, &tray, &state);
            if let Ok(mut current) = current.lock() {
                *current = Some(state.clone());
            }
            let _ = app.emit(STATE_EVENT, state);

            if state_wake.recv_timeout(REFRESH_INTERVAL).is_ok() {
                while state_wake.try_recv().is_ok() {}
            }
        }
    });
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(SharedUsage::default())
        .invoke_handler(tauri::generate_handler![current_usage])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.handle()
                .set_activation_policy(tauri::ActivationPolicy::Accessory)?;

            let detecting =
                MenuItem::with_id(app, "detecting", "Detecting...", false, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&detecting, &separator, &quit])?;
            let current = app.state::<SharedUsage>().inner().clone();
            let activity = sidecar::SharedActivity::default();
            let (state_wake_tx, state_wake_rx) = mpsc::channel();
            let activity_wake = sidecar::spawn_activity_loop(
                app.handle().clone(),
                activity.clone(),
                state_wake_tx.clone(),
            );
            let panel_state = current.clone();
            let panel_activity = activity.clone();
            let panel_state_wake = state_wake_tx.clone();
            let panel_activity_wake = activity_wake.clone();
            let panel_visible_on_press = AtomicBool::new(false);
            let tray = TrayIconBuilder::with_id("main")
                .title("Loading...")
                .tooltip(APP_NAME)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "quit" {
                        app.exit(0);
                    }
                })
                .on_tray_icon_event(move |tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state,
                        rect,
                        ..
                    } = event
                    {
                        match button_state {
                            MouseButtonState::Down => {
                                let visible = tray
                                    .app_handle()
                                    .get_webview_window(PANEL_LABEL)
                                    .is_some_and(|panel| panel.is_visible().unwrap_or(false));
                                panel_visible_on_press.store(visible, Ordering::Relaxed);
                            }
                            MouseButtonState::Up => {
                                if panel_visible_on_press.swap(false, Ordering::Relaxed) {
                                    if let Some(panel) =
                                        tray.app_handle().get_webview_window(PANEL_LABEL)
                                    {
                                        let _ = panel.hide();
                                    }
                                } else {
                                    show_panel(
                                        tray.app_handle(),
                                        rect,
                                        &panel_state,
                                        &panel_state_wake,
                                        &panel_activity,
                                        &panel_activity_wake,
                                    );
                                }
                            }
                        }
                    }
                })
                .build(app)?;

            spawn_state_loop(
                app.handle().clone(),
                tray,
                current,
                state_wake_rx,
                state_wake_tx,
                activity,
            );
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == PANEL_LABEL && matches!(event, WindowEvent::Focused(false)) {
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run BlockWatcher");
}

#[cfg(test)]
mod tests {
    use super::*;
    use blockwatcher_core::{CodexRateWindow, ModelActivity, TokenCounts};

    fn timestamp(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn entry(at: &str, tokens: u64) -> UsageEntry {
        UsageEntry {
            timestamp: timestamp(at),
            model: Some("claude-sonnet".into()),
            tokens: TokenCounts {
                output: tokens,
                ..Default::default()
            },
            message_id: None,
            request_id: None,
        }
    }

    #[test]
    fn tray_prefers_the_running_provider_and_omits_installation_type() {
        let now = timestamp("2026-07-17T10:30:00Z");
        let claude = claude_state(
            Installation {
                cli: true,
                app: false,
                data: true,
                running: true,
            },
            None,
            &[
                entry("2026-07-17T00:15:00Z", 100),
                entry("2026-07-17T10:15:00Z", 50),
            ],
            2,
            now,
        );
        let codex = codex_state(
            Installation {
                cli: false,
                app: true,
                data: true,
                running: false,
            },
            Some(&CodexUsage {
                observed_at: timestamp("2026-07-17T10:20:00Z"),
                plan_type: Some("plus".into()),
                windows: vec![CodexRateWindow {
                    used_percent: 58.0,
                    window_minutes: 10_080,
                    resets_at: timestamp("2026-07-17T15:30:00Z"),
                }],
            }),
            None,
            None,
            0,
            now,
        );

        let state = usage_state(vec![claude, codex]);
        assert_eq!(state.title, "Claude · 50%");
        assert_eq!(state.providers[0].summary, "4:30 · 50%");
        assert_eq!(state.providers[1].summary, "58% · 5:00");
        assert_eq!(provider_menu_text(&state.providers[0]), "Claude · 50%");
        assert_eq!(provider_menu_text(&state.providers[1]), "Codex · 58%");
        assert!(state.providers.iter().all(|provider| provider.installed));
    }

    #[test]
    fn claude_exposes_panel_estimates_and_models() {
        let now = timestamp("2026-07-17T10:30:00Z");
        let snapshot = claude_state(
            Installation {
                cli: true,
                app: false,
                data: true,
                running: true,
            },
            None,
            &[
                entry("2026-07-17T00:00:00Z", 1_000),
                entry("2026-07-17T10:00:00Z", 100),
                entry("2026-07-17T10:10:00Z", 100),
            ],
            0,
            now,
        );

        assert_eq!(snapshot.state.models[0].tokens, 200);
        assert_eq!(snapshot.state.burn_rate, Some(20.0));
        assert_eq!(snapshot.state.projected_tokens, Some(5_600));
        assert_eq!(
            snapshot.state.eta_at.as_deref(),
            Some("2026-07-17T11:10:00+00:00")
        );
    }

    #[test]
    fn claude_plan_usage_remains_authoritative_and_keeps_models() {
        let now = timestamp("2026-07-18T12:00:00Z");
        let usage = ClaudeUsage {
            observed_at: timestamp("2026-07-18T10:54:26Z"),
            windows: vec![
                blockwatcher_core::ClaudeRateWindow {
                    used_percent: 100.0,
                    window_minutes: 300,
                },
                blockwatcher_core::ClaudeRateWindow {
                    used_percent: 48.0,
                    window_minutes: 10_080,
                },
            ],
        };
        let snapshot = claude_state(
            Installation {
                cli: true,
                app: true,
                data: true,
                running: true,
            },
            Some(&usage),
            &[UsageEntry {
                model: Some("claude-fable-5".into()),
                ..entry("2026-07-18T11:30:00Z", 10)
            }],
            0,
            now,
        );

        assert_eq!(provider_menu_text(&snapshot.state), "Claude · 100%");
        assert_eq!(snapshot.state.windows.len(), 2);
        assert_eq!(snapshot.state.windows[1].used_percent, Some(48.0));
        assert!(!snapshot.state.windows[0].estimated);
        assert_eq!(snapshot.state.windows[0].reset_at, None);
        assert_eq!(snapshot.state.tokens, None);
        assert_eq!(snapshot.state.models[0].name, "Fable 5");
        assert_eq!(snapshot.state.models[0].tokens, 10);
    }

    #[test]
    fn codex_keeps_real_limits_and_adds_daily_activity() {
        let now = timestamp("2026-07-18T12:00:00Z");
        let activity = AgentActivity {
            id: "codex".into(),
            total_tokens: 1_200,
            tokens: TokenCounts {
                input: 200,
                output: 100,
                cache_read: 900,
                ..Default::default()
            },
            models: vec![ModelActivity {
                name: "gpt-5.6-sol".into(),
                tokens: TokenCounts {
                    input: 200,
                    output: 100,
                    cache_read: 900,
                    ..Default::default()
                },
            }],
        };
        let snapshot = codex_state(
            Installation {
                cli: true,
                app: true,
                data: true,
                running: true,
            },
            Some(&CodexUsage {
                observed_at: now,
                plan_type: None,
                windows: vec![CodexRateWindow {
                    used_percent: 66.0,
                    window_minutes: 300,
                    resets_at: timestamp("2026-07-18T14:00:00Z"),
                }],
            }),
            Some(&activity),
            Some(now),
            0,
            now,
        );

        assert_eq!(snapshot.state.windows[0].used_percent, Some(66.0));
        assert_eq!(snapshot.state.tokens, Some(1_200));
        assert_eq!(snapshot.state.models[0].name, "gpt-5.6-sol");
    }

    #[test]
    fn activity_provider_has_tokens_without_an_invented_window() {
        let activity = AgentActivity {
            id: "hermes".into(),
            total_tokens: 500,
            tokens: TokenCounts {
                input: 300,
                output: 200,
                ..Default::default()
            },
            models: Vec::new(),
        };
        let snapshot = generic_state(
            &providers::PROVIDERS[7],
            Installation {
                cli: true,
                app: false,
                data: true,
                running: false,
            },
            Some(&activity),
            Some(timestamp("2026-07-18T12:00:00Z")),
        );

        assert_eq!(snapshot.state.name, "Hermes");
        assert_eq!(snapshot.state.tokens, Some(500));
        assert!(snapshot.state.windows.is_empty());
        assert_eq!(provider_menu_text(&snapshot.state), "Hermes · 500");
    }

    #[test]
    fn providers_sort_core_first_then_open_then_alphabetically() {
        let installed = |definition: &providers::ProviderDefinition, running| {
            generic_state(
                definition,
                Installation {
                    cli: true,
                    app: false,
                    data: false,
                    running,
                },
                None,
                None,
            )
        };
        let state = usage_state(vec![
            installed(&providers::PROVIDERS[24], false),
            installed(&providers::PROVIDERS[18], true),
            installed(&providers::PROVIDERS[2], true),
            installed(&providers::PROVIDERS[17], true),
        ]);

        assert_eq!(
            state
                .providers
                .iter()
                .map(|provider| provider.id.as_str())
                .collect::<Vec<_>>(),
            ["chatgpt", "antigravity", "windsurf", "continue"]
        );
        assert!(state.providers[0].windows.is_empty());
        assert_eq!(state.providers[0].tokens, None);
    }

    #[test]
    fn panel_position_stays_inside_negative_monitor_coordinates() {
        assert_eq!(clamp_panel_coordinate(-2_000.0, -1_920, 1_920, 360), -1_912);
        assert_eq!(clamp_panel_coordinate(-100.0, -1_920, 1_920, 360), -368);
        assert_eq!(clamp_panel_coordinate(900.0, 0, 1_000, 360), 632);
    }
}
