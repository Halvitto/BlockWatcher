use std::{
    collections::HashSet,
    env,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Debug)]
pub(crate) struct ProviderDefinition {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) icon: &'static str,
    pub(crate) commands: &'static [&'static str],
    pub(crate) app_names: &'static [&'static str],
    pub(crate) bundle_ids: &'static [&'static str],
    pub(crate) processes: &'static [&'static str],
    pub(crate) ccusage_id: Option<&'static str>,
}

pub(crate) const PROVIDERS: &[ProviderDefinition] = &[
    provider(
        "claude",
        "Claude",
        &["claude"],
        &["Claude.app", "Claude Code.app"],
        &[
            "com.anthropic.claudefordesktop",
            "com.anthropic.claude-code",
        ],
        &["Claude", "claude"],
        Some("claude"),
    ),
    provider(
        "codex",
        "Codex",
        &["codex"],
        &["Codex.app"],
        &["com.openai.codex"],
        &["Codex", "codex"],
        Some("codex"),
    ),
    provider(
        "chatgpt",
        "ChatGPT",
        &[],
        &["ChatGPT.app", "ChatGPT Classic.app"],
        &["com.openai.chat", "com.openai.codex"],
        &["ChatGPT"],
        None,
    ),
    provider(
        "opencode",
        "OpenCode",
        &["opencode"],
        &["OpenCode.app"],
        &[],
        &["OpenCode", "opencode"],
        Some("opencode"),
    ),
    provider("amp", "Amp", &["amp"], &[], &[], &["amp"], Some("amp")),
    provider(
        "droid",
        "Droid",
        &["droid"],
        &["Droid.app", "Factory.app"],
        &[],
        &["Droid", "droid"],
        Some("droid"),
    ),
    provider(
        "codebuff",
        "Codebuff",
        &["codebuff"],
        &["Codebuff.app"],
        &[],
        &["Codebuff", "codebuff"],
        Some("codebuff"),
    ),
    provider(
        "hermes",
        "Hermes",
        &["hermes"],
        &["Hermes.app"],
        &[],
        &["Hermes", "hermes"],
        Some("hermes"),
    ),
    provider("pi", "pi-agent", &["pi"], &[], &[], &["pi"], Some("pi")),
    provider(
        "goose",
        "Goose",
        &["goose"],
        &["Goose.app"],
        &[],
        &["Goose", "goose"],
        Some("goose"),
    ),
    provider(
        "openclaw",
        "OpenClaw",
        &["openclaw", "clawdbot", "moltbot", "moldbot"],
        &["OpenClaw.app"],
        &[],
        &["OpenClaw", "openclaw", "clawdbot", "moltbot", "moldbot"],
        Some("openclaw"),
    ),
    provider(
        "kilo",
        "Kilo",
        &["kilo", "kilocode", "kilo-code"],
        &["Kilo.app", "Kilo Code.app"],
        &[],
        &["Kilo", "Kilo Code", "kilo"],
        Some("kilo"),
    ),
    provider(
        "copilot",
        "GitHub Copilot CLI",
        &["copilot"],
        &["GitHub Copilot.app"],
        &[],
        &["GitHub Copilot", "copilot"],
        Some("copilot"),
    ),
    provider(
        "gemini",
        "Gemini CLI",
        &["gemini"],
        &["Gemini.app"],
        &[],
        &["Gemini", "gemini"],
        Some("gemini"),
    ),
    provider(
        "kimi",
        "Kimi",
        &["kimi", "kimi-code"],
        &["Kimi.app", "Kimi Code.app"],
        &[],
        &["Kimi", "Kimi Code", "kimi", "kimi-code"],
        Some("kimi"),
    ),
    provider(
        "qwen",
        "Qwen",
        &["qwen", "qwen-code"],
        &["Qwen.app", "Qwen Code.app"],
        &[],
        &["Qwen", "Qwen Code", "qwen"],
        Some("qwen"),
    ),
    provider(
        "cursor",
        "Cursor",
        &["cursor"],
        &["Cursor.app"],
        &["com.todesktop.230313mzl4w4u92"],
        &["Cursor", "cursor"],
        None,
    ),
    provider(
        "windsurf",
        "Windsurf",
        &["windsurf"],
        &["Windsurf.app"],
        &["com.exafunction.windsurf"],
        &["Windsurf", "windsurf"],
        None,
    ),
    provider(
        "antigravity",
        "Antigravity",
        &["agy", "antigravity"],
        &["Antigravity.app"],
        &["com.google.antigravity"],
        &["Antigravity", "agy", "antigravity"],
        None,
    ),
    provider(
        "kiro",
        "Kiro",
        &["kiro"],
        &["Kiro.app"],
        &[],
        &["Kiro", "kiro"],
        None,
    ),
    provider(
        "trae",
        "Trae",
        &["trae"],
        &["Trae.app"],
        &[],
        &["Trae", "trae"],
        None,
    ),
    provider(
        "zed",
        "Zed",
        &["zed"],
        &["Zed.app"],
        &["dev.zed.Zed"],
        &["Zed", "zed"],
        None,
    ),
    provider(
        "warp",
        "Warp",
        &["warp"],
        &["Warp.app"],
        &["dev.warp.Warp-Stable"],
        &["Warp", "warp"],
        None,
    ),
    provider("aider", "Aider", &["aider"], &[], &[], &["aider"], None),
    provider(
        "continue",
        "Continue",
        &["continue", "cn"],
        &["Continue.app"],
        &[],
        &["Continue", "continue", "cn"],
        None,
    ),
];

const fn provider(
    id: &'static str,
    name: &'static str,
    commands: &'static [&'static str],
    app_names: &'static [&'static str],
    bundle_ids: &'static [&'static str],
    processes: &'static [&'static str],
    ccusage_id: Option<&'static str>,
) -> ProviderDefinition {
    ProviderDefinition {
        id,
        name,
        icon: id,
        commands,
        app_names,
        bundle_ids,
        processes,
        ccusage_id,
    }
}

#[derive(Default)]
pub(crate) struct AppInventory {
    names: HashSet<String>,
    bundle_ids: HashSet<String>,
}

impl AppInventory {
    pub(crate) fn discover() -> Self {
        let mut inventory = Self::default();
        let mut roots = vec![PathBuf::from("/Applications")];
        if let Some(home) = env::var_os("HOME") {
            roots.push(Path::new(&home).join("Applications"));
        }

        for root in roots {
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() || path.extension().is_none_or(|extension| extension != "app") {
                    continue;
                }
                if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                    inventory.names.insert(name.to_string());
                }
                let info = path.join("Contents/Info.plist");
                let Ok(output) = Command::new("/usr/bin/plutil")
                    .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
                    .arg(info)
                    .output()
                else {
                    continue;
                };
                if output.status.success()
                    && let Ok(bundle_id) = String::from_utf8(output.stdout)
                {
                    inventory.bundle_ids.insert(bundle_id.trim().to_string());
                }
            }
        }
        inventory
    }

    pub(crate) fn contains(&self, provider: &ProviderDefinition) -> bool {
        provider
            .app_names
            .iter()
            .any(|name| self.names.contains(*name))
            || provider
                .bundle_ids
                .iter()
                .any(|bundle_id| self.bundle_ids.contains(*bundle_id))
    }

    #[cfg(test)]
    pub(crate) fn from_values(names: &[&str], bundle_ids: &[&str]) -> Self {
        Self {
            names: names.iter().map(|value| value.to_string()).collect(),
            bundle_ids: bundle_ids.iter().map(|value| value.to_string()).collect(),
        }
    }
}

#[derive(Default)]
pub(crate) struct ProcessInventory {
    names: HashSet<String>,
    commands: Vec<String>,
}

impl ProcessInventory {
    pub(crate) fn discover() -> Self {
        let Ok(output) = Command::new("/bin/ps")
            .args(["-axww", "-o", "comm="])
            .output()
        else {
            return Self::default();
        };
        let commands = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        let names = commands
            .iter()
            .map(String::as_str)
            .filter_map(|line| Path::new(line.trim()).file_name())
            .filter_map(|name| name.to_str())
            .map(str::to_string)
            .collect();
        Self { names, commands }
    }

    pub(crate) fn contains(&self, provider: &ProviderDefinition) -> bool {
        provider
            .processes
            .iter()
            .any(|name| self.names.contains(*name))
    }

    pub(crate) fn app_executable_is_open(&self, app_name: &str, executable: &str) -> bool {
        let suffix = format!("/{app_name}/Contents/MacOS/{executable}");
        self.commands
            .iter()
            .any(|command| command.ends_with(&suffix))
    }

    #[cfg(test)]
    pub(crate) fn from_values(names: &[&str]) -> Self {
        Self {
            names: names.iter().map(|value| value.to_string()).collect(),
            commands: names.iter().map(|value| value.to_string()).collect(),
        }
    }
}

pub(crate) fn command_installed(provider: &ProviderDefinition) -> bool {
    let directories = executable_directories();
    command_installed_in(provider, &directories)
}

fn command_installed_in(provider: &ProviderDefinition, directories: &[PathBuf]) -> bool {
    provider.commands.iter().any(|name| {
        directories
            .iter()
            .any(|directory| executable_file(&directory.join(name)))
    })
}

fn executable_directories() -> Vec<PathBuf> {
    let mut directories = env::var_os("PATH")
        .map(|path| env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    if let Some(home) = env::var_os("HOME") {
        let home = Path::new(&home);
        directories.extend(
            [
                ".local/bin",
                ".bun/bin",
                ".npm-global/bin",
                ".volta/bin",
                ".asdf/shims",
                ".local/share/mise/shims",
                "Library/pnpm",
            ]
            .map(|path| home.join(path)),
        );
    }
    directories
        .extend(["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"].map(PathBuf::from));
    directories
}

fn executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn chatgpt_and_codex_share_only_the_integrated_bundle() {
        let classic = AppInventory::from_values(&[], &["com.openai.chat"]);
        assert!(classic.contains(&PROVIDERS[2]));
        assert!(!classic.contains(&PROVIDERS[1]));

        let integrated = AppInventory::from_values(&[], &["com.openai.codex"]);
        assert!(integrated.contains(&PROVIDERS[1]));
        assert!(integrated.contains(&PROVIDERS[2]));
    }

    #[test]
    fn process_inventory_ignores_helper_names() {
        let processes =
            ProcessInventory::from_values(&["Antigravity Helper (Renderer)", "ChatGPT", "codex"]);
        assert!(!processes.contains(&PROVIDERS[18]));
        assert!(processes.contains(&PROVIDERS[1]));
        assert!(processes.contains(&PROVIDERS[2]));
    }

    #[test]
    fn integrated_chatgpt_process_is_distinct_from_classic() {
        let integrated = ProcessInventory {
            names: HashSet::from(["ChatGPT".into()]),
            commands: vec!["/Applications/ChatGPT.app/Contents/MacOS/ChatGPT".into()],
        };
        assert!(integrated.app_executable_is_open("ChatGPT.app", "ChatGPT"));
        assert!(!integrated.app_executable_is_open("ChatGPT Classic.app", "ChatGPT"));
    }

    #[test]
    fn registry_has_unique_ids_and_all_expected_sources() {
        let ids = PROVIDERS
            .iter()
            .map(|provider| provider.id)
            .collect::<HashSet<_>>();
        let icons = PROVIDERS
            .iter()
            .map(|provider| provider.icon)
            .collect::<HashSet<_>>();
        assert_eq!(ids.len(), 25);
        assert_eq!(icons, ids);
        assert_eq!(
            PROVIDERS
                .iter()
                .filter(|provider| provider.ccusage_id.is_some())
                .count(),
            15
        );
    }

    #[cfg(unix)]
    #[test]
    fn executable_detection_uses_real_files_on_path() {
        let directory =
            env::temp_dir().join(format!("blockwatcher-provider-test-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let executable = directory.join("hermes");
        fs::write(&executable, "#!/bin/sh\n").unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();

        assert!(command_installed_in(
            &PROVIDERS[7],
            std::slice::from_ref(&directory)
        ));
        assert!(!command_installed_in(
            &PROVIDERS[6],
            std::slice::from_ref(&directory)
        ));

        fs::remove_dir_all(directory).unwrap();
    }
}
