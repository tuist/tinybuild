//! Planning and execution.
//!
//! A task's identity (its cache key) is a hash of everything that can change
//! its output: the platform, the script, the fingerprint of every declared
//! tool, every declared environment value, the content of every declared input
//! file, and the *output* hash of every dependency. That last part is what
//! gives early cutoff: a dependency that reruns but produces identical bytes
//! does not change its dependents' keys.
//!
//! Execution is hermetic. Each task runs in a fresh sandbox containing only its
//! declared inputs and its dependencies' outputs, with a scrubbed environment.
//! A task that reads something it did not declare simply will not find it.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use sha2::{Digest, Sha256};

use crate::graph::Graph;
use crate::store::{hex, manifest_hash, Manifest, Store};
use crate::task::Task;

const ENV_ALLOWLIST: &[&str] = &["PATH", "HOME", "TMPDIR", "DEVELOPER_DIR"];

pub struct Summary {
    pub ran: usize,
    pub cached: usize,
    pub keys: Vec<String>,
}

fn targets(graph: &Graph, target: Option<&str>) -> Result<Vec<String>, String> {
    match target {
        Some(t) => {
            if !graph.tasks.contains_key(t) {
                return Err(format!("unknown task `{t}`"));
            }
            Ok(vec![t.to_string()])
        }
        None => Ok(graph.roots()),
    }
}

pub fn run(
    graph: &Graph,
    target: Option<&str>,
    root: &Path,
    store: &Store,
    out_dir: &Path,
) -> Result<Summary, String> {
    let wanted = targets(graph, target)?;
    let mut subset = std::collections::HashSet::new();
    for t in &wanted {
        subset.extend(graph.closure(t)?);
    }
    let waves = graph.waves(&subset)?;

    let manifests: Mutex<HashMap<String, Manifest>> = Mutex::new(HashMap::new());
    let mut ran = 0;
    let mut cached = 0;
    let mut keys = Vec::new();

    for wave in waves {
        let snapshot = manifests.lock().unwrap().clone();
        let snapshot = &snapshot;

        let results: Vec<Result<(String, String, Manifest, bool), String>> =
            std::thread::scope(|scope| {
                let handles: Vec<_> = wave
                    .iter()
                    .map(|id| scope.spawn(move || realize(graph, id, snapshot, root, store)))
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            });

        let mut guard = manifests.lock().unwrap();
        for result in results {
            let (id, key, manifest, hit) = result?;
            if hit {
                cached += 1;
                println!("  CACHED  {id}");
            } else {
                ran += 1;
                println!("  RUN     {id}");
            }
            keys.push(key);
            guard.insert(id, manifest);
        }
    }

    let final_manifests = manifests.into_inner().unwrap();
    for t in &wanted {
        if let Some(manifest) = final_manifests.get(t) {
            for (decl, leaves) in manifest {
                store.materialize(out_dir, decl, leaves)?;
            }
        }
    }

    Ok(Summary { ran, cached, keys })
}

fn realize(
    graph: &Graph,
    id: &str,
    dep_manifests: &HashMap<String, Manifest>,
    root: &Path,
    store: &Store,
) -> Result<(String, String, Manifest, bool), String> {
    let task = &graph.tasks[id];
    let key = compute_key(task, graph, dep_manifests, root)?;

    if let Some(manifest) = store.get_action(&key) {
        return Ok((id.to_string(), key, manifest, true));
    }

    let sandbox = store.sandbox_for(id);
    let _ = std::fs::remove_dir_all(&sandbox);
    std::fs::create_dir_all(&sandbox).map_err(|e| e.to_string())?;

    for pattern in &task.inputs {
        for path in glob_inputs(root, pattern)? {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let dst = sandbox.join(rel);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::copy(&path, &dst).map_err(|e| e.to_string())?;
        }
    }

    for dep in graph.dependencies(id) {
        let manifest = dep_manifests
            .get(dep)
            .ok_or_else(|| format!("dependency `{dep}` was not realized before `{id}`"))?;
        for (decl, leaves) in manifest {
            store.materialize(&sandbox, decl, leaves)?;
        }
    }

    let status = Command::new("bash")
        .arg(&task.path)
        .current_dir(&sandbox)
        .env_clear()
        .envs(scrubbed_env(task))
        .status()
        .map_err(|e| format!("failed to start `{id}`: {e}"))?;
    if !status.success() {
        return Err(format!("task `{id}` failed with {status}"));
    }

    let mut manifest = Manifest::new();
    for decl in &task.outputs {
        let leaves = store.put_output(&sandbox, decl)?;
        manifest.insert(decl.clone(), leaves);
    }
    store.put_action(&key, &manifest)?;

    Ok((id.to_string(), key, manifest, false))
}

fn compute_key(
    task: &Task,
    graph: &Graph,
    dep_manifests: &HashMap<String, Manifest>,
    root: &Path,
) -> Result<String, String> {
    let mut hasher = Sha256::new();

    // The platform is an input: the same source built on a different OS or
    // architecture is a different result.
    hasher.update(b"platform\0");
    hasher.update(std::env::consts::OS.as_bytes());
    hasher.update(b"/");
    hasher.update(std::env::consts::ARCH.as_bytes());

    hasher.update(b"script\0");
    hasher.update(&task.source);

    // Tools are inputs too. Their fingerprint changes when the toolchain does.
    let mut tools = task.tools.clone();
    tools.sort();
    for tool in tools {
        hasher.update(b"tool\0");
        hasher.update(tool.as_bytes());
        hasher.update(b"\0");
        hasher.update(fingerprint_tool(&tool)?.as_bytes());
    }

    let mut env = task.env.clone();
    env.sort();
    for name in env {
        hasher.update(b"env\0");
        hasher.update(name.as_bytes());
        hasher.update(b"=");
        hasher.update(std::env::var(&name).unwrap_or_default().as_bytes());
    }

    for pattern in &task.inputs {
        for path in glob_inputs(root, pattern)? {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let bytes = std::fs::read(&path)
                .map_err(|e| format!("cannot read input {}: {e}", path.display()))?;
            hasher.update(b"input\0");
            hasher.update(rel.to_string_lossy().as_bytes());
            hasher.update(b"\0");
            hasher.update(crate::store::hash_bytes(&bytes).as_bytes());
        }
    }

    // Depend on each dependency's *output* hash, not its key. This is what
    // makes invalidation stop early when a rebuilt output is unchanged.
    let mut deps = graph.dependencies(&task.id).to_vec();
    deps.sort();
    for dep in deps {
        let manifest = dep_manifests
            .get(&dep)
            .ok_or_else(|| format!("dependency `{dep}` not realized before `{}`", task.id))?;
        hasher.update(b"dep\0");
        hasher.update(dep.as_bytes());
        hasher.update(b"\0");
        hasher.update(manifest_hash(manifest).as_bytes());
    }

    Ok(hex(hasher.finalize().as_slice()))
}

fn fingerprint_tool(cmd: &str) -> Result<String, String> {
    let output = Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .env_clear()
        .envs(base_env())
        .output()
        .map_err(|e| format!("tool `{cmd}` failed to run: {e}"))?;
    let mut bytes = output.stdout;
    bytes.extend_from_slice(&output.stderr);
    bytes.push(output.status.code().unwrap_or(-1) as u8);
    Ok(crate::store::hash_bytes(&bytes))
}

fn glob_inputs(root: &Path, pattern: &str) -> Result<Vec<std::path::PathBuf>, String> {
    let joined = root.join(pattern);
    let pattern = joined.to_string_lossy();
    let mut matched = Vec::new();
    for entry in glob::glob(&pattern).map_err(|e| format!("bad input glob `{pattern}`: {e}"))? {
        let path = entry.map_err(|e| e.to_string())?;
        if path.is_file() {
            matched.push(path);
        }
    }
    matched.sort();
    matched.dedup();
    Ok(matched)
}

fn base_env() -> Vec<(String, String)> {
    ENV_ALLOWLIST
        .iter()
        .filter_map(|name| std::env::var(name).ok().map(|v| (name.to_string(), v)))
        .collect()
}

fn scrubbed_env(task: &Task) -> Vec<(String, String)> {
    let mut env = base_env();
    for name in &task.env {
        if let Ok(value) = std::env::var(name) {
            env.push((name.clone(), value));
        }
    }
    env
}

pub fn plan(graph: &Graph, target: Option<&str>) -> Result<String, String> {
    let wanted = targets(graph, target)?;
    let mut subset = std::collections::HashSet::new();
    for t in &wanted {
        subset.extend(graph.closure(t)?);
    }
    let waves = graph.waves(&subset)?;

    let mut entries = Vec::new();
    for wave in &waves {
        for id in wave {
            let task = &graph.tasks[id];
            entries.push(serde_json::json!({
                "id": task.id,
                "command": ["bash", task.id],
                "needs": task.needs,
                "inputs": task.inputs,
                "outputs": task.outputs,
                "env": task.env,
                "tools": task.tools,
            }));
        }
    }

    serde_json::to_string_pretty(&serde_json::json!({
        "platform": format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH),
        "tasks": entries,
    }))
    .map_err(|e| e.to_string())
}
