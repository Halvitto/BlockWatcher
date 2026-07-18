import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  disable as disableAutostart,
  enable as enableAutostart,
  isEnabled as isAutostartEnabled,
} from "@tauri-apps/plugin-autostart";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { useEffect, useMemo, useRef, useState } from "react";
import aiderIcon from "./assets/providers/aider.png";
import ampIcon from "./assets/providers/amp-color.svg";
import antigravityIcon from "./assets/providers/antigravity-color.svg";
import claudeIcon from "./assets/providers/claude-color.svg";
import codebuffIcon from "./assets/providers/codebuff.png";
import codexIcon from "./assets/providers/codex-color.svg";
import continueIcon from "./assets/providers/continue.png";
import cursorIcon from "./assets/providers/cursor.svg";
import droidIcon from "./assets/providers/droid.png";
import geminiIcon from "./assets/providers/geminicli-color.svg";
import copilotIcon from "./assets/providers/githubcopilot.svg";
import gooseIcon from "./assets/providers/goose.svg";
import hermesIcon from "./assets/providers/hermesagent.svg";
import kiloIcon from "./assets/providers/kilocode.svg";
import kimiIcon from "./assets/providers/kimi-color.svg";
import kiroIcon from "./assets/providers/kiro-color.svg";
import openaiIcon from "./assets/providers/openai.svg";
import openclawIcon from "./assets/providers/openclaw-color.svg";
import opencodeIcon from "./assets/providers/opencode.svg";
import piIcon from "./assets/providers/pi.svg";
import qwenIcon from "./assets/providers/qwen-color.svg";
import traeIcon from "./assets/providers/trae-color.svg";
import warpIcon from "./assets/providers/warp.png";
import windsurfIcon from "./assets/providers/windsurf.svg";
import zedIcon from "./assets/providers/zed.jpg";
import {
  usageTransitions,
  type UsageWindowSnapshot,
} from "./notifications";

type RateWindow = {
  usedPercent: number | null;
  windowMinutes: number;
  remainingMinutes: number | null;
  resetAt: string | null;
  estimated: boolean;
};

type ModelUsage = {
  name: string;
  tokens: number;
  input: number;
  output: number;
  cacheCreation: number;
  cacheRead: number;
};

type Provider = {
  id: string;
  name: string;
  icon: string;
  installed: boolean;
  running: boolean;
  active: boolean;
  summary: string;
  windows: RateWindow[];
  tokens: number | null;
  input: number | null;
  output: number | null;
  cacheCreation: number | null;
  cacheRead: number | null;
  estimatedLimit: number | null;
  models: ModelUsage[];
  burnRate: number | null;
  projectedTokens: number | null;
  etaAt: string | null;
  malformedLines: number;
  updatedAt: string | null;
};

const PROVIDER_ICONS: Readonly<Record<string, string>> = {
  claude: claudeIcon,
  codex: codexIcon,
  chatgpt: openaiIcon,
  opencode: opencodeIcon,
  amp: ampIcon,
  droid: droidIcon,
  codebuff: codebuffIcon,
  hermes: hermesIcon,
  pi: piIcon,
  goose: gooseIcon,
  openclaw: openclawIcon,
  kilo: kiloIcon,
  copilot: copilotIcon,
  gemini: geminiIcon,
  kimi: kimiIcon,
  qwen: qwenIcon,
  cursor: cursorIcon,
  windsurf: windsurfIcon,
  antigravity: antigravityIcon,
  kiro: kiroIcon,
  trae: traeIcon,
  zed: zedIcon,
  warp: warpIcon,
  aider: aiderIcon,
  continue: continueIcon,
};

type UsageState = {
  title: string;
  providers: Provider[];
};

type NotificationSettings = {
  limitAlerts: boolean;
  resetAlerts: boolean;
};

const SETTINGS_KEY = "blockwatcher.settings";
const DEFAULT_SETTINGS: NotificationSettings = {
  limitAlerts: false,
  resetAlerts: false,
};

function loadNotificationSettings(): NotificationSettings {
  try {
    const value = JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? "null");
    if (typeof value !== "object" || value === null) return DEFAULT_SETTINGS;
    return {
      limitAlerts:
        typeof value.limitAlerts === "boolean" ? value.limitAlerts : false,
      resetAlerts:
        typeof value.resetAlerts === "boolean" ? value.resetAlerts : false,
    };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

const compactNumber = new Intl.NumberFormat(undefined, {
  notation: "compact",
  maximumFractionDigits: 1,
});

const exactNumber = new Intl.NumberFormat();

const timeFormat = new Intl.DateTimeFormat(undefined, {
  hour: "numeric",
  minute: "2-digit",
});

const dateTimeFormat = new Intl.DateTimeFormat(undefined, {
  weekday: "short",
  hour: "numeric",
  minute: "2-digit",
});

function demoState(): UsageState {
  const fromNow = (minutes: number) =>
    new Date(Date.now() + minutes * 60_000).toISOString();

  return {
    title: "Claude · 100%",
    providers: [
      {
        id: "claude",
        name: "Claude",
        icon: "claude",
        installed: true,
        running: false,
        active: true,
        summary: "100%",
        windows: [
          {
            usedPercent: 100,
            windowMinutes: 300,
            remainingMinutes: null,
            resetAt: null,
            estimated: false,
          },
          {
            usedPercent: 48,
            windowMinutes: 10_080,
            remainingMinutes: null,
            resetAt: null,
            estimated: false,
          },
        ],
        tokens: null,
        input: null,
        output: null,
        cacheCreation: null,
        cacheRead: null,
        estimatedLimit: null,
        models: [
          {
            name: "Fable 5",
            tokens: 6_945_011,
            input: 243,
            output: 131_578,
            cacheCreation: 943_717,
            cacheRead: 5_869_473,
          },
        ],
        burnRate: null,
        projectedTokens: null,
        etaAt: null,
        malformedLines: 0,
        updatedAt: new Date().toISOString(),
      },
      {
        id: "codex",
        name: "Codex",
        icon: "codex",
        installed: true,
        running: true,
        active: true,
        summary: "66% · 2:48",
        windows: [
          {
            usedPercent: 66,
            windowMinutes: 300,
            remainingMinutes: 168,
            resetAt: fromNow(168),
            estimated: false,
          },
          {
            usedPercent: 24,
            windowMinutes: 10_080,
            remainingMinutes: 7_820,
            resetAt: fromNow(7_820),
            estimated: false,
          },
        ],
        tokens: 75_541_348,
        input: 2_360_167,
        output: 382_589,
        cacheCreation: 0,
        cacheRead: 72_798_592,
        estimatedLimit: null,
        models: [
          {
            name: "gpt-5.6-sol",
            tokens: 65_946_796,
            input: 1_939_164,
            output: 348_112,
            cacheCreation: 0,
            cacheRead: 63_659_520,
          },
        ],
        burnRate: null,
        projectedTokens: null,
        etaAt: null,
        malformedLines: 0,
        updatedAt: new Date().toISOString(),
      },
      {
        id: "chatgpt",
        name: "ChatGPT",
        icon: "chatgpt",
        installed: true,
        running: true,
        active: true,
        summary: "Open",
        windows: [],
        tokens: null,
        input: null,
        output: null,
        cacheCreation: null,
        cacheRead: null,
        estimatedLimit: null,
        models: [],
        burnRate: null,
        projectedTokens: null,
        etaAt: null,
        malformedLines: 0,
        updatedAt: null,
      },
      {
        id: "hermes",
        name: "Hermes",
        icon: "hermes",
        installed: true,
        running: false,
        active: true,
        summary: "Today",
        windows: [],
        tokens: 128_400,
        input: 96_000,
        output: 22_400,
        cacheCreation: 0,
        cacheRead: 10_000,
        estimatedLimit: null,
        models: [
          {
            name: "Hermes 4",
            tokens: 128_400,
            input: 96_000,
            output: 22_400,
            cacheCreation: 0,
            cacheRead: 10_000,
          },
        ],
        burnRate: null,
        projectedTokens: null,
        etaAt: null,
        malformedLines: 0,
        updatedAt: new Date().toISOString(),
      },
    ],
  };
}

function windowLabel(minutes: number) {
  if (minutes === 300) return "5-hour session";
  if (minutes === 10_080) return "Weekly";
  if (minutes % 1_440 === 0) return `${minutes / 1_440}-day window`;
  if (minutes % 60 === 0) return `${minutes / 60}-hour window`;
  return `${minutes}-minute window`;
}

function countdown(resetAt: string, now: number) {
  const totalSeconds = Math.max(
    0,
    Math.floor((new Date(resetAt).getTime() - now) / 1_000),
  );
  const days = Math.floor(totalSeconds / 86_400);
  const hours = Math.floor((totalSeconds % 86_400) / 3_600);
  const minutes = Math.floor((totalSeconds % 3_600) / 60);
  const seconds = totalSeconds % 60;

  if (days > 0) return `${days}d ${hours}h`;
  return `${hours}:${minutes.toString().padStart(2, "0")}:${seconds
    .toString()
    .padStart(2, "0")}`;
}

function resetLabel(resetAt: string, now: number) {
  const reset = new Date(resetAt);
  return reset.getTime() - now >= 86_400_000
    ? dateTimeFormat.format(reset)
    : timeFormat.format(reset);
}

function WindowRow({ value, now }: { value: RateWindow; now: number }) {
  const percent = value.usedPercent;
  const progress = Math.min(100, Math.max(0, percent ?? 0));

  return (
    <div className="window-row">
      <div className="window-heading">
        <span>{windowLabel(value.windowMinutes)}</span>
        <span className="window-percent">
          {percent === null ? "No limit data" : `${Math.round(percent)}%`}
          {value.estimated && <span className="estimate-tag">Est.</span>}
        </span>
      </div>
      {percent !== null && (
        <div
          className="progress"
          role="progressbar"
          aria-label={`${windowLabel(value.windowMinutes)} usage`}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(progress)}
        >
          <div className="progress-fill" style={{ width: `${progress}%` }} />
        </div>
      )}
      {value.resetAt ? (
        <div className="window-timing">
          <span>Resets {resetLabel(value.resetAt, now)}</span>
          <time dateTime={value.resetAt}>{countdown(value.resetAt, now)}</time>
        </div>
      ) : (
        <div className="window-timing">
          <span>Reset not provided</span>
        </div>
      )}
    </div>
  );
}

function Metrics({ provider }: { provider: Provider }) {
  const cache =
    provider.cacheCreation !== null && provider.cacheRead !== null
      ? provider.cacheCreation + provider.cacheRead
      : null;
  const rows = [
    provider.tokens !== null
      ? ["Today", exactNumber.format(provider.tokens)]
      : null,
    provider.input !== null
      ? ["Input", exactNumber.format(provider.input)]
      : null,
    provider.output !== null
      ? ["Output", exactNumber.format(provider.output)]
      : null,
    cache !== null
      ? ["Cache", exactNumber.format(cache)]
      : null,
    provider.estimatedLimit !== null
      ? ["Est. limit", compactNumber.format(provider.estimatedLimit)]
      : null,
    provider.burnRate !== null
      ? ["Burn rate (est.)", `${compactNumber.format(provider.burnRate)}/min`]
      : null,
    provider.projectedTokens !== null
      ? ["Projection (est.)", compactNumber.format(provider.projectedTokens)]
      : null,
    provider.etaAt !== null
      ? ["Est. limit time", timeFormat.format(new Date(provider.etaAt))]
      : null,
  ].filter((row): row is string[] => row !== null);

  if (rows.length === 0) return null;

  return (
    <dl className="metrics">
      {rows.map(([label, value]) => (
        <div key={label}>
          <dt>{label}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}

function ModelList({ models }: { models: ModelUsage[] }) {
  if (models.length === 0) return null;

  return (
    <div className="models">
      <h3>Model activity</h3>
      {models.map((model) => (
        <details className="model" key={model.name}>
          <summary className="model-summary">
            <strong>{model.name}</strong>
            <span className="chevron" aria-hidden="true" />
          </summary>
          <dl className="model-metrics">
            <div>
              <dt>Total</dt>
              <dd>{compactNumber.format(model.tokens)}</dd>
            </div>
            <div>
              <dt>Output</dt>
              <dd>{compactNumber.format(model.output)}</dd>
            </div>
            <div>
              <dt>Cache</dt>
              <dd>
                {compactNumber.format(model.cacheCreation + model.cacheRead)}
              </dd>
            </div>
          </dl>
        </details>
      ))}
    </div>
  );
}

function ProviderSection({
  provider,
  now,
}: {
  provider: Provider;
  now: number;
}) {
  const primary = provider.windows[0];
  const percent = primary?.usedPercent;
  const hasQuota = percent !== null && percent !== undefined;
  const hasActivity = provider.tokens !== null;
  const expandable =
    provider.windows.length > 1 ||
    provider.models.length > 0 ||
    hasActivity ||
    primary?.resetAt != null;
  const progress = Math.min(100, Math.max(0, percent ?? 0));
  const updatedAt = provider.updatedAt
    ? new Date(provider.updatedAt)
    : null;
  const stale =
    updatedAt !== null &&
    Number.isFinite(updatedAt.getTime()) &&
    now - updatedAt.getTime() > 15 * 60_000;
  const status = provider.running ? "Open" : "Installed";

  return (
    <details
      className={`provider provider--${provider.id} ${expandable ? "" : "provider--static"}`}
    >
      <summary className="provider-summary">
        <div className="provider-heading">
          <span className="provider-mark">
            <img src={PROVIDER_ICONS[provider.icon]} alt="" />
          </span>
          <div className="provider-title">
            <div className="provider-name">
              <h2>{provider.name}</h2>
              {(primary || hasActivity) && (
                <span>
                  {primary
                    ? windowLabel(primary.windowMinutes)
                    : "Tokens today"}
                </span>
              )}
            </div>
            {provider.running && (
              <span
                className="running-dot"
                role="img"
                aria-label="Open now"
                title="Open now"
              />
            )}
          </div>
          {hasQuota && (
            <strong className="provider-percent">
              {Math.round(percent)}%
              {primary.estimated && (
                <span className="estimate-tag">Est.</span>
              )}
            </strong>
          )}
          {!hasQuota && hasActivity && (
            <strong className="provider-tokens">
              {compactNumber.format(provider.tokens ?? 0)}
            </strong>
          )}
          {!hasQuota && !hasActivity && (
            <span className="provider-status">{status}</span>
          )}
          {expandable && <span className="chevron" aria-hidden="true" />}
        </div>
        {hasQuota && (
          <div
            className="progress summary-progress"
            role="progressbar"
            aria-label={`${provider.name} ${windowLabel(primary.windowMinutes)} usage`}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(progress)}
          >
            <div className="progress-fill" style={{ width: `${progress}%` }} />
          </div>
        )}
      </summary>

      {expandable && (
        <div className="provider-details">
          {primary?.resetAt && (
            <div className="window-timing primary-timing">
              <span>Resets {resetLabel(primary.resetAt, now)}</span>
              <time dateTime={primary.resetAt}>
                {countdown(primary.resetAt, now)}
              </time>
            </div>
          )}

          {stale && updatedAt && (
            <p className="source-age">
              Last updated {timeFormat.format(updatedAt)}
            </p>
          )}

          {provider.windows.length > 1 && (
            <div className="windows">
              {provider.windows.slice(1).map((window) => (
                <WindowRow
                  key={`${window.windowMinutes}-${window.resetAt ?? "unknown"}`}
                  value={window}
                  now={now}
                />
              ))}
            </div>
          )}

          <ModelList models={provider.models} />
          <Metrics provider={provider} />

          {provider.malformedLines > 0 && (
            <p className="warning">
              {exactNumber.format(provider.malformedLines)} malformed lines
              skipped
            </p>
          )}
        </div>
      )}
    </details>
  );
}

function SettingsPanel({
  notifications,
  launchAtLogin,
  busy,
  error,
  onNotificationChange,
  onLaunchChange,
}: {
  notifications: NotificationSettings;
  launchAtLogin: boolean | null;
  busy: boolean;
  error: string | null;
  onNotificationChange: (
    key: keyof NotificationSettings,
    enabled: boolean,
  ) => void;
  onLaunchChange: (enabled: boolean) => void;
}) {
  return (
    <details className="settings">
      <summary className="settings-summary">
        <strong>Settings</strong>
        <span className="chevron" aria-hidden="true" />
      </summary>
      <div className="settings-content">
        <label className="setting-row">
          <span>
            <strong>80% alert</strong>
            <small>Notify when a usage window reaches 80%</small>
          </span>
          <input
            className="setting-toggle"
            type="checkbox"
            checked={notifications.limitAlerts}
            disabled={busy}
            onChange={(event) =>
              onNotificationChange("limitAlerts", event.currentTarget.checked)
            }
          />
        </label>
        <label className="setting-row">
          <span>
            <strong>Reset alert</strong>
            <small>Notify when a usage window resets</small>
          </span>
          <input
            className="setting-toggle"
            type="checkbox"
            checked={notifications.resetAlerts}
            disabled={busy}
            onChange={(event) =>
              onNotificationChange("resetAlerts", event.currentTarget.checked)
            }
          />
        </label>
        <label className="setting-row">
          <span>
            <strong>Launch at login</strong>
            <small>Open BlockWatcher after sign-in</small>
          </span>
          <input
            className="setting-toggle"
            type="checkbox"
            checked={launchAtLogin ?? false}
            disabled={busy || launchAtLogin === null}
            onChange={(event) => onLaunchChange(event.currentTarget.checked)}
          />
        </label>
        {error && (
          <p className="settings-error" role="status">
            {error}
          </p>
        )}
      </div>
    </details>
  );
}

export default function App() {
  const tauri = isTauri();
  const [state, setState] = useState<UsageState | null>(() =>
    tauri ? null : demoState(),
  );
  const [loadFailed, setLoadFailed] = useState(false);
  const [focused, setFocused] = useState(() => document.hasFocus());
  const [now, setNow] = useState(Date.now());
  const [notificationSettings, setNotificationSettings] = useState(
    loadNotificationSettings,
  );
  const [launchAtLogin, setLaunchAtLogin] = useState<boolean | null>(
    tauri ? null : false,
  );
  const [settingsBusy, setSettingsBusy] = useState(false);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const usageSnapshots = useRef<ReadonlyMap<string, UsageWindowSnapshot>>(
    new Map(),
  );
  const hasUsageSnapshot = useRef(false);
  const notificationSettingsRef = useRef(notificationSettings);
  notificationSettingsRef.current = notificationSettings;

  useEffect(() => {
    if (!tauri) return;
    let disposed = false;
    let stop: (() => void) | undefined;

    async function connect() {
      let listenFailed = false;
      try {
        const unlisten = await listen<UsageState>("usage-state", (event) => {
          setState(event.payload);
          setLoadFailed(false);
        });
        if (disposed) {
          unlisten();
          return;
        }
        stop = unlisten;
      } catch {
        listenFailed = true;
      }

      if (disposed) return;
      try {
        const initial = await invoke<UsageState | null>("current_usage");
        if (!disposed && initial !== null) setState(initial);
        if (!disposed && initial === null && listenFailed) setLoadFailed(true);
      } catch {
        if (!disposed) setLoadFailed(true);
      }
    }

    void connect();
    return () => {
      disposed = true;
      stop?.();
    };
  }, [tauri]);

  useEffect(() => {
    const onFocus = () => setFocused(true);
    const onBlur = () => setFocused(false);
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
    };
  }, []);

  useEffect(() => {
    if (!focused) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [focused]);

  useEffect(() => {
    try {
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(notificationSettings));
    } catch {
      // The settings remain usable for this session when storage is unavailable.
    }
  }, [notificationSettings]);

  useEffect(() => {
    if (!tauri) return;
    let disposed = false;
    void isAutostartEnabled()
      .then((enabled) => {
        if (!disposed) setLaunchAtLogin(enabled);
      })
      .catch(() => {
        if (!disposed) {
          setLaunchAtLogin(false);
          setSettingsError("Unable to read launch-at-login status.");
        }
      });
    return () => {
      disposed = true;
    };
  }, [tauri]);

  useEffect(() => {
    if (!tauri || state === null) return;
    const transition = usageTransitions(usageSnapshots.current, state.providers);
    usageSnapshots.current = transition.snapshots;
    if (!hasUsageSnapshot.current) {
      hasUsageSnapshot.current = true;
      return;
    }

    const settings = notificationSettingsRef.current;
    const notices = transition.notices.filter((notice) =>
      notice.kind === "limit" ? settings.limitAlerts : settings.resetAlerts,
    );
    if (notices.length === 0) return;

    void isPermissionGranted()
      .then((granted) => {
        if (!granted) return;
        for (const notice of notices) {
          sendNotification({
            title: `${notice.providerName} usage`,
            body:
              notice.kind === "limit"
                ? `${windowLabel(notice.windowMinutes)} reached ${Math.round(notice.usedPercent ?? 0)}%.`
                : `${windowLabel(notice.windowMinutes)} has reset.`,
          });
        }
      })
      .catch(() => {});
  }, [state, tauri]);

  async function changeNotificationSetting(
    key: keyof NotificationSettings,
    enabled: boolean,
  ) {
    setSettingsError(null);
    if (enabled && tauri) {
      setSettingsBusy(true);
      try {
        const granted =
          (await isPermissionGranted()) ||
          (await requestPermission()) === "granted";
        if (!granted) {
          setSettingsError("Notification permission was not granted.");
          return;
        }
      } catch {
        setSettingsError("Unable to request notification permission.");
        return;
      } finally {
        setSettingsBusy(false);
      }
    }
    setNotificationSettings((current) => ({ ...current, [key]: enabled }));
  }

  async function changeLaunchAtLogin(enabled: boolean) {
    setSettingsError(null);
    if (!tauri) {
      setLaunchAtLogin(enabled);
      return;
    }
    setSettingsBusy(true);
    try {
      await (enabled ? enableAutostart() : disableAutostart());
      setLaunchAtLogin(enabled);
    } catch {
      setSettingsError("Unable to update launch at login.");
    } finally {
      setSettingsBusy(false);
    }
  }

  const providers = useMemo(
    () => state?.providers.filter((provider) => provider.installed) ?? [],
    [state],
  );
  const runningCount = providers.filter((provider) => provider.running).length;
  const activityLabel =
    state === null
      ? loadFailed
        ? "Unavailable"
        : "Reading"
      : runningCount > 0
        ? "Open"
        : "Idle";

  return (
    <main
      className="panel"
      tabIndex={0}
      aria-busy={state === null && !loadFailed}
    >
      <header className="app-header">
        <div className="app-identity">
          <div>
            <h1>BlockWatcher</h1>
            <p>
              {state
                ? `${runningCount} open · ${providers.length} detected`
                : "Reading local usage"}
            </p>
          </div>
        </div>
        <span
          className={`live-indicator ${state !== null && runningCount > 0 ? "active" : ""}`}
        >
          <span aria-hidden="true" />
          {activityLabel}
        </span>
      </header>

      {state === null && !loadFailed && (
        <div className="loading" role="status">
          <span className="sr-only">Reading local usage</span>
          <span className="loading-dots" aria-hidden="true">
            <i />
            <i />
            <i />
          </span>
        </div>
      )}

      {state === null && loadFailed && (
        <div className="empty-state" role="alert">
          <strong>Unable to read local usage</strong>
        </div>
      )}

      {state !== null && providers.length === 0 && (
        <div className="empty-state" role="status">
          <strong>No supported clients detected</strong>
        </div>
      )}

      {providers.map((provider) => (
        <ProviderSection key={provider.id} provider={provider} now={now} />
      ))}

      <SettingsPanel
        notifications={notificationSettings}
        launchAtLogin={launchAtLogin}
        busy={settingsBusy}
        error={settingsError}
        onNotificationChange={(key, enabled) => {
          void changeNotificationSetting(key, enabled);
        }}
        onLaunchChange={(enabled) => {
          void changeLaunchAtLogin(enabled);
        }}
      />
    </main>
  );
}
