# BlockWatcher

BlockWatcher is an open-source macOS menu bar app that shows local quota and
token activity for AI coding clients. It detects installed apps and CLIs,
highlights clients that are open, and works without an account, API key,
telemetry, or runtime network access.

> Status: early development. Local builds work; signed and notarized releases
> are not available yet.

## What It Shows

| Level | Providers | Data |
| --- | --- | --- |
| Quota and activity | Claude, Codex | Real provider quota windows plus local models and tokens |
| Activity | OpenCode, Amp, Droid, Codebuff, Hermes, pi-agent, Goose, OpenClaw, Kilo, Kimi, Qwen, GitHub Copilot CLI, Gemini CLI | Today's local tokens and models; no invented percentage |
| Detection | ChatGPT, Cursor, Windsurf, Antigravity, Kiro, Trae, Zed, Warp, Aider, Continue | `Open` or `Installed` |

Claude always uses the real five-hour session percentage in its compact row.
Expanding it shows Weekly and current model activity. Claude Desktop plan
history is authoritative; Claude Code session blocks are an estimated fallback
only when that history is missing or invalid.

Codex percentages come only from local Codex rate-limit records. Daily Codex
models and tokens are merged in separately and never used to derive a quota.
ChatGPT is intentionally a separate detection-only row.

## Features

- Detects supported clients from `PATH`, macOS app bundles, running processes,
  or valid local activity.
- Shows only detected providers, with Claude, Codex, and ChatGPT first.
- Uses expandable native rows for providers, models, and Settings.
- Refreshes local activity at startup, every five minutes, and on panel open
  when the snapshot is older than one minute.
- Provides optional quota notifications and launch at login.
- Bundles the pinned Rust `ccusage` binary as a Tauri sidecar.
- Uses provider-specific logos and supports light and dark appearance.

## Privacy

BlockWatcher runs its activity scanner with:

```text
ccusage daily --json --by-agent --offline --no-cost
```

The sidecar also receives an empty config and an empty build-time pricing
snapshot, so it does not fetch pricing or make runtime network requests.
BlockWatcher scans local session records but extracts only installation state,
timestamps, model identifiers, token counts, and rate-limit metadata. It does
not retain or display prompts or responses.

Primary local sources include:

| Source | Path |
| --- | --- |
| Claude plan usage | `~/Library/Application Support/Claude/plan-usage-history.json` |
| Claude Code fallback | `~/.claude/projects/**/*.jsonl` or `CLAUDE_CONFIG_DIR` |
| Codex limits and activity | `$CODEX_HOME/sessions/**/*.jsonl` or `~/.codex/sessions/**/*.jsonl` |
| Other activity providers | Provider-specific local paths supported by the bundled `ccusage` |

## Run Locally

Requirements: macOS, Rust 1.85 or newer, Node.js, npm, and Xcode Command Line
Tools.

Clone with the pinned sidecar source:

```bash
git clone --recurse-submodules <repository-url> BlockWatcher
cd BlockWatcher/app
npm install
npm run tauri dev
```

For an existing clone:

```bash
git submodule update --init --recursive
```

`npm run tauri dev` compiles `vendor/ccusage` at the pinned commit, copies the
target-named sidecar into `app/src-tauri/binaries`, starts Vite, and launches
the menu bar app.

## Build

```bash
cd app
npm install
npm run tauri build
```

The sidecar build uses the vendored Rust workspace and its lockfile. Generated
sidecar binaries are ignored by Git.

Architecture-specific builds use the target passed by Tauri:

```bash
npm run tauri build -- --target aarch64-apple-darwin
npm run tauri build -- --target x86_64-apple-darwin
```

## Development

```text
core/             Pure Rust parsing and usage logic
app/              Tauri 2 shell and React panel
vendor/ccusage/   Pinned git submodule used to build the sidecar
```

Run all checks:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cd app
npm test
npm run build
```

See [SPEC.md](SPEC.md) for architecture and behavior, and [NOTICE](NOTICE) for
third-party code, icon provenance, and trademark notices.

## Contributing

Issues and focused pull requests are welcome. Preserve offline operation, do
not expose conversation content, and include a runnable test for non-trivial
parsing or detection changes.

## License

BlockWatcher is available under the [MIT License](LICENSE).
