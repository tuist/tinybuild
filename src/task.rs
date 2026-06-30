//! Parsing of task files.
//!
//! A task is just a shell script that carries its contract in header comments:
//!
//! ```sh
//! #!/usr/bin/env bash
//! # tinybuild needs compile
//! # tinybuild input src/**/*.txt
//! # tinybuild output build/app.txt
//! # tinybuild env NAME
//! ```
//!
//! The script body stays a normal script. The `# tinybuild` lines are the only
//! thing the build system reads to know how to schedule and cache it.

use std::fs;
use std::path::{Path, PathBuf};

const PREFIX: &str = "# tinybuild ";

/// A single unit of work in the graph.
#[derive(Debug, Clone)]
pub struct Task {
    /// Derived from the file name, e.g. `tasks/compile.sh` -> `compile`.
    pub name: String,
    pub path: PathBuf,
    /// Raw script bytes. Part of the cache key: edit the script, rerun the task.
    pub source: Vec<u8>,
    /// Names of tasks that must run before this one.
    pub needs: Vec<String>,
    /// Globs whose matched files decide whether the work changed.
    pub inputs: Vec<String>,
    /// Paths the task is expected to produce. Restored on a cache hit.
    pub outputs: Vec<String>,
    /// Environment variables forwarded to the script and folded into the key.
    pub env: Vec<String>,
}

impl Task {
    fn parse(path: &Path, source: Vec<u8>) -> Result<Task, String> {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("invalid task file name: {}", path.display()))?
            .to_string();

        let text = String::from_utf8_lossy(&source);
        let mut task = Task {
            name,
            path: path.to_path_buf(),
            source: source.clone(),
            needs: Vec::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            env: Vec::new(),
        };

        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix(PREFIX) {
                let mut parts = rest.splitn(2, char::is_whitespace);
                let verb = parts.next().unwrap_or("").trim();
                let arg = parts.next().unwrap_or("").trim().to_string();
                if arg.is_empty() {
                    return Err(format!("`{PREFIX}{verb}` is missing its value"));
                }
                match verb {
                    "needs" => task.needs.push(arg),
                    "input" => task.inputs.push(arg),
                    "output" => task.outputs.push(arg),
                    "env" => task.env.push(arg),
                    other => return Err(format!("unknown directive `{other}`")),
                }
            }
        }

        Ok(task)
    }
}

/// Load every `*.sh` file in `dir` as a task.
pub fn load(dir: &Path) -> Result<Vec<Task>, String> {
    let mut tasks = Vec::new();
    let entries =
        fs::read_dir(dir).map_err(|e| format!("cannot read tasks dir {}: {e}", dir.display()))?;

    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("sh") {
            continue;
        }
        let source = fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let task = Task::parse(&path, source).map_err(|e| format!("{}: {e}", path.display()))?;
        tasks.push(task);
    }

    tasks.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(tasks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(name: &str, body: &str) -> Result<Task, String> {
        Task::parse(
            &PathBuf::from(format!("tasks/{name}.sh")),
            body.as_bytes().to_vec(),
        )
    }

    #[test]
    fn parses_every_directive() {
        let task = parse(
            "build",
            "#!/usr/bin/env bash\n\
             # tinybuild needs compile\n\
             # tinybuild input src/**/*.txt\n\
             # tinybuild output build/out.txt\n\
             # tinybuild env NAME\n\
             echo hi\n",
        )
        .unwrap();

        assert_eq!(task.name, "build");
        assert_eq!(task.needs, ["compile"]);
        assert_eq!(task.inputs, ["src/**/*.txt"]);
        assert_eq!(task.outputs, ["build/out.txt"]);
        assert_eq!(task.env, ["NAME"]);
    }

    #[test]
    fn ignores_lines_without_the_prefix() {
        let task = parse("noop", "echo hi\n# a normal comment\n").unwrap();
        assert!(task.needs.is_empty());
        assert!(task.inputs.is_empty());
    }

    #[test]
    fn rejects_unknown_directive() {
        let err = parse("bad", "# tinybuild frobnicate x\n").unwrap_err();
        assert!(err.contains("unknown directive"));
    }

    #[test]
    fn rejects_directive_without_value() {
        assert!(parse("bad", "# tinybuild needs\n").is_err());
    }
}
