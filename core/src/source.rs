//! Log discovery. All filesystem layout knowledge lives behind [`LogSource`]
//! so the future Windows/WSL implementations (v0.2) are purely additive.

use std::path::{Path, PathBuf};

/// Where a supported client's session logs live.
pub trait LogSource {
    /// Directories to watch for log changes.
    fn roots(&self) -> Vec<PathBuf>;

    /// All session log files under the roots, sorted for determinism.
    fn log_files(&self) -> Vec<PathBuf>;
}

impl LogSource for MacLogSource {
    fn roots(&self) -> Vec<PathBuf> {
        self.roots
            .iter()
            .map(|root| root.join("projects"))
            .collect()
    }

    fn log_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for root in &self.roots {
            collect_jsonl(&root.join("projects"), &mut files);
        }
        files.sort();
        files
    }
}

/// macOS log source. Honors `CLAUDE_CONFIG_DIR` (comma-separated paths, `~`
/// expanded) and otherwise uses `$XDG_CONFIG_HOME/claude` and `~/.claude` —
/// both, like ccusage, since Claude Code has used each over time.
#[derive(Debug, Clone)]
pub struct MacLogSource {
    roots: Vec<PathBuf>,
}

impl MacLogSource {
    /// Discover roots from the environment. Empty when none exist.
    pub fn discover() -> Self {
        Self {
            roots: discover_roots(
                std::env::var("CLAUDE_CONFIG_DIR").ok().as_deref(),
                std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
                std::env::var("HOME").ok().map(PathBuf::from).as_deref(),
            ),
        }
    }
}

/// Codex state source. Honors `CODEX_HOME` and otherwise uses `~/.codex`.
#[derive(Debug, Clone)]
pub struct CodexLogSource {
    root: Option<PathBuf>,
}

impl CodexLogSource {
    pub fn discover() -> Self {
        let root = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".codex"))
            })
            .filter(|path| path.is_dir());
        Self { root }
    }
}

impl LogSource for CodexLogSource {
    fn roots(&self) -> Vec<PathBuf> {
        self.root.iter().map(|root| root.join("sessions")).collect()
    }

    fn log_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for root in self.roots() {
            collect_jsonl(&root, &mut files);
        }
        files.sort();
        files
    }
}

/// Pure discovery logic, split from env access for testability.
fn discover_roots(
    config_dir: Option<&str>,
    xdg_config_home: Option<&str>,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let push = |mut path: PathBuf, roots: &mut Vec<PathBuf>| {
        if path.file_name().is_some_and(|name| name == "projects") && path.is_dir() {
            path.pop();
        }
        if path.join("projects").is_dir() && !roots.contains(&path) {
            roots.push(path);
        }
    };
    if let Some(dirs) = config_dir {
        for raw in dirs.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            push(expand_home(raw, home), &mut roots);
        }
        // An explicit override wins even when none of its paths exist.
        return roots;
    }
    let Some(home) = home else {
        return roots;
    };
    let xdg = xdg_config_home
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    push(xdg.join("claude"), &mut roots);
    push(home.join(".claude"), &mut roots);
    roots
}

fn expand_home(raw: &str, home: Option<&Path>) -> PathBuf {
    match (raw.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(raw),
    }
}

fn collect_jsonl(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            collect_jsonl(&path, files);
        } else if file_type.is_file() && path.extension().is_some_and(|e| e == "jsonl") {
            files.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "blockwatcher-src-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn config_dir_override_takes_precedence_and_splits_on_commas() {
        let base = scratch("override");
        let a = base.join("a");
        let b = base.join("b");
        std::fs::create_dir_all(a.join("projects")).unwrap();
        std::fs::create_dir_all(b.join("projects")).unwrap();
        std::fs::create_dir_all(base.join("home/.claude/projects")).unwrap();

        let spec = format!(
            " {} , {} ,,{}",
            a.display(),
            b.display(),
            base.join("missing").display()
        );
        let roots = discover_roots(Some(&spec), None, Some(&base.join("home")));
        assert_eq!(roots, vec![a, b]); // missing dir dropped, home ignored
    }

    #[test]
    fn falls_back_to_xdg_and_home_dirs() {
        let base = scratch("fallback");
        let home = base.join("home");
        std::fs::create_dir_all(home.join(".config/claude/projects")).unwrap();
        std::fs::create_dir_all(home.join(".claude/projects")).unwrap();

        let roots = discover_roots(None, None, Some(&home));
        assert_eq!(
            roots,
            vec![home.join(".config/claude"), home.join(".claude")]
        );
    }

    #[test]
    fn tilde_expansion_in_override() {
        let base = scratch("tilde");
        let home = base.join("home");
        std::fs::create_dir_all(home.join("custom/projects")).unwrap();
        let roots = discover_roots(Some("~/custom"), None, Some(&home));
        assert_eq!(roots, vec![home.join("custom")]);
    }

    #[test]
    fn config_dir_accepts_projects_directory() {
        let root = scratch("projects-override").join("config");
        let projects = root.join("projects");
        std::fs::create_dir_all(&projects).unwrap();

        let roots = discover_roots(Some(&projects.to_string_lossy()), None, None);
        assert_eq!(roots, vec![root]);
    }

    #[test]
    fn log_files_walks_projects_recursively_and_sorts() {
        let base = scratch("walk");
        let root = base.join("cfg");
        std::fs::create_dir_all(root.join("projects/proj-b")).unwrap();
        std::fs::create_dir_all(root.join("projects/proj-a/nested")).unwrap();
        std::fs::write(root.join("projects/proj-b/2.jsonl"), "").unwrap();
        std::fs::write(root.join("projects/proj-a/nested/1.jsonl"), "").unwrap();
        std::fs::write(root.join("projects/proj-a/ignore.txt"), "").unwrap();
        std::fs::write(root.join("not-projects.jsonl"), "").unwrap();

        let source = MacLogSource {
            roots: vec![root.clone()],
        };
        assert_eq!(
            source.log_files(),
            vec![
                root.join("projects/proj-a/nested/1.jsonl"),
                root.join("projects/proj-b/2.jsonl"),
            ]
        );
    }
}
