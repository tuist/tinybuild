//! Parsing and loading of task files.
//!
//! A task is a shell script that declares its contract in header comments:
//!
//! ```sh
//! #!/usr/bin/env bash
//! # tinybuild needs ./compile.sh
//! # tinybuild input Sources/main.swift
//! # tinybuild output MyApp.app
//! # tinybuild env CONFIGURATION
//! # tinybuild tool swiftc --version
//! ```
//!
//! A task is identified by its script path, and `needs` points at another
//! script by relative path, so the graph is a graph of files. Inputs and
//! outputs are relative to the project root. `tool` declares a command whose
//! output fingerprints a tool, so the toolchain becomes a real input.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const PREFIX: &str = "# tinybuild ";

#[derive(Debug, Clone)]
pub struct Task {
    /// Path of the script relative to the project root, e.g. `tasks/compile.sh`.
    pub id: String,
    /// Absolute path to the script.
    pub path: PathBuf,
    pub source: Vec<u8>,
    /// Ids of tasks that must run first.
    pub needs: Vec<String>,
    /// Source globs, relative to the project root.
    pub inputs: Vec<String>,
    /// Declared outputs, relative to the project root.
    pub outputs: Vec<String>,
    pub env: Vec<String>,
    /// Commands whose output identifies a tool, e.g. `swiftc --version`.
    pub tools: Vec<String>,
}

fn normalize_id(root: &Path, path: &Path) -> Result<String, String> {
    let canon = fs::canonicalize(path).map_err(|_| format!("cannot resolve {}", path.display()))?;
    let rel = canon
        .strip_prefix(root)
        .map_err(|_| format!("{} is outside the project", path.display()))?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

fn parse(root: &Path, path: &Path) -> Result<Task, String> {
    let abs = fs::canonicalize(path).map_err(|_| format!("cannot resolve {}", path.display()))?;
    let source = fs::read(&abs).map_err(|e| format!("cannot read {}: {e}", abs.display()))?;
    let id = normalize_id(root, &abs)?;
    let dir = abs.parent().unwrap_or(Path::new(".")).to_path_buf();

    let mut task = Task {
        id,
        path: abs.clone(),
        source: source.clone(),
        needs: Vec::new(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        env: Vec::new(),
        tools: Vec::new(),
    };

    for line in String::from_utf8_lossy(&source).lines() {
        let Some(rest) = line.trim().strip_prefix(PREFIX) else {
            continue;
        };
        let mut parts = rest.splitn(2, char::is_whitespace);
        let verb = parts.next().unwrap_or("").trim();
        let arg = parts.next().unwrap_or("").trim().to_string();
        if arg.is_empty() {
            return Err(format!("`{PREFIX}{verb}` is missing its value"));
        }
        match verb {
            "needs" => {
                let need_path = dir.join(&arg);
                let need_id = normalize_id(root, &need_path)
                    .map_err(|_| format!("needs `{arg}`, which does not exist"))?;
                task.needs.push(need_id);
            }
            "input" => task.inputs.push(arg),
            "output" => task.outputs.push(arg),
            "env" => task.env.push(arg),
            "tool" => task.tools.push(arg),
            other => return Err(format!("unknown directive `{other}`")),
        }
    }

    Ok(task)
}

/// Resolve a script path to its task id (its path relative to the project root).
pub fn id_for(root: &Path, path: &Path) -> Result<String, String> {
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    normalize_id(&root, path)
}

/// Load tasks, following `needs` edges to pull in their dependencies.
///
/// With explicit `targets` (script paths), the graph is whatever those targets
/// reach. With none, tinybuild discovers tasks by scanning the project for
/// `*.sh` files that carry a `# tinybuild` header, so the scripts can live in
/// any directory, `tasks/`, `build/`, `scripts/`, it does not matter.
pub fn load(root: &Path, targets: &[PathBuf]) -> Result<Vec<Task>, String> {
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let mut queue: Vec<PathBuf> = Vec::new();
    if targets.is_empty() {
        discover(&root, &mut queue)?;
    } else {
        queue.extend(targets.iter().cloned());
    }

    let mut map: BTreeMap<String, Task> = BTreeMap::new();
    while let Some(path) = queue.pop() {
        let id = normalize_id(&root, &path)?;
        if map.contains_key(&id) {
            continue;
        }
        let task = parse(&root, &path).map_err(|e| format!("{}: {e}", path.display()))?;
        for need in &task.needs {
            queue.push(root.join(need));
        }
        map.insert(id, task);
    }

    Ok(map.into_values().collect())
}

/// Walk the project for `*.sh` files that declare a `# tinybuild` directive.
fn discover(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if path.is_dir() {
            if name.starts_with('.') || matches!(name, "out" | "target" | "node_modules") {
                continue;
            }
            discover(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("sh") {
            let text = fs::read_to_string(&path).unwrap_or_default();
            if text.contains(PREFIX) {
                out.push(path);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(dir: &Path, name: &str, body: &str) {
        fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn parses_directives_and_resolves_needs_by_path() {
        let tmp = std::env::temp_dir().join(format!("tinybuild-test-{}", std::process::id()));
        let tasks = tmp.join("tasks");
        fs::create_dir_all(&tasks).unwrap();
        write(&tasks, "compile.sh", "# tinybuild input Sources/main.swift\n# tinybuild output MyApp\n# tinybuild tool swiftc --version\n");
        write(
            &tasks,
            "bundle.sh",
            "# tinybuild needs ./compile.sh\n# tinybuild output MyApp.app\n",
        );

        let loaded = load(&tmp, &[]).unwrap();
        let bundle = loaded.iter().find(|t| t.id == "tasks/bundle.sh").unwrap();
        assert_eq!(bundle.needs, ["tasks/compile.sh"]);

        let compile = loaded.iter().find(|t| t.id == "tasks/compile.sh").unwrap();
        assert_eq!(compile.inputs, ["Sources/main.swift"]);
        assert_eq!(compile.outputs, ["MyApp"]);
        assert_eq!(compile.tools, ["swiftc --version"]);

        let _ = fs::remove_dir_all(&tmp);
    }
}
