# BlockWatcher Multi-Agent Spec

## Product

BlockWatcher is an open-source macOS menu bar utility for local AI client
quota, activity, and installation status. Runtime behavior is offline.

## Support Matrix

### Quota and activity

- **Claude:** five-hour and Weekly percentages from Claude Desktop plan history;
  Claude Code block estimate only as fallback; active-block models.
- **Codex:** provider-reported rate-limit windows from `~/.codex`; today's
  models and tokens from `ccusage`.

### Activity

The bundled `ccusage` supplies daily token and model activity for Claude,
Codex, OpenCode, Amp, Droid, Codebuff, Hermes, pi-agent, Goose, OpenClaw,
Kilo, Kimi, Qwen, GitHub Copilot CLI, and Gemini CLI.

Non-quota providers must never display a percentage or progress bar.

### Detection only

ChatGPT, Cursor, Windsurf, Antigravity, Kiro, Trae, Zed, Warp, Aider, and
Continue display only `Open` or `Installed`.

`com.openai.chat` detects ChatGPT only. `com.openai.codex` detects ChatGPT and
integrated Codex. Codex percentages still come exclusively from Codex local
rate-limit records.

## Architecture

```text
core/             Platform-neutral Rust parsing and calculations
app/src-tauri/    Detection, sidecar lifecycle, watchers, tray, IPC
app/src/          React panel
vendor/ccusage/   Git submodule pinned to
                  7acee6c5853c26fe66fbe1453bd94c9376afec06
```

`core` has no Tauri or UI dependency. Provider installation definitions are a
static registry in the Tauri layer because app bundles and processes are
platform concerns.

## Activity Sidecar

Tauri packages the release `ccusage` binary through `bundle.externalBin`.
`app/scripts/prepare-sidecar.mjs` compiles the pinned Rust workspace with its
lockfile and an empty pricing snapshot.

Each read executes:

```text
ccusage daily --json --by-agent --offline --no-cost \
  --since <local-day> --until <local-day> --config <empty-config>
```

Rules:

- Run immediately, every five minutes, and on panel open when older than
  60 seconds.
- Use one worker, so sidecar executions cannot overlap.
- Kill a read after 10 seconds.
- Replace shared activity only after valid JSON is parsed.
- Preserve the last valid snapshot on timeout, non-zero exit, or invalid data.
- Ignore unknown fields and malformed agent/model rows.

## Detection

Each `ProviderDefinition` includes ID, display name, executable names, `.app`
names, bundle IDs, process names, optional `ccusage` adapter, and UI icon.

- Installation: executable file on known `PATH`, app bundle in `/Applications`
  or `~/Applications`, or valid local activity.
- Open state: one `ps` snapshot per state refresh, with exact main executable
  names so helper processes do not count.
- The integrated ChatGPT executable counts as open Codex only when launched
  from `ChatGPT.app`.

Detected providers are ordered Claude, Codex, ChatGPT; then open providers;
then the rest alphabetically. Undetected providers are omitted.

## UI

All provider, model, and Settings disclosures use native
`<details>/<summary>` and begin closed.

- Quota row: logo, name, open indicator, primary percentage, minimal bar.
- Activity row: logo, name, open indicator, today's tokens; no bar.
- Detection row: logo, name, `Open` or `Installed`.
- Expanded quota: secondary windows, timing, models, and available activity.
- Expanded activity: input, output, cache, total, and models.
- Settings is always the final row.
- Old valid data shows `Last updated` rather than being replaced by an
  estimate.

The native tray menu is rebuilt from detected providers and never includes
installation labels such as `(CLI + APP)`.

## Privacy

BlockWatcher makes no runtime network requests. Session files are processed
locally and only usage metadata is retained in memory. Conversation content is
not retained, displayed, or logged.

## Acceptance

- Claude current fixture: 100% five-hour, 48% Weekly, Fable 5 details.
- Codex: local quota percentages plus daily models/tokens.
- Hermes: daily tokens and models with no percentage or bar.
- ChatGPT and Antigravity: detection state only.
- PATH, bundle, process, helper exclusion, timeout, invalid output, and
  last-snapshot behavior are tested.
- Native disclosures are keyboard focusable.
- No horizontal overflow at `300x620` or `360x620`, light or dark.
- Workspace tests, Clippy, frontend tests/build, and Tauri sidecar build pass.
