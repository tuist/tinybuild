//! Scheduling and execution.
//!
//! Walk the graph one wave at a time. Within a wave every task is independent,
//! so we run them on separate threads. Before running a task we compute its key
//! and ask the cache: on a hit we restore its outputs and skip the work, on a
//! miss we run the script and store the result.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use crate::cache;
use crate::graph::Graph;
use crate::task::Task;

pub struct Outcome {
    pub hit: bool,
}

pub fn run(graph: &Graph, target: Option<&str>, workspace: &Path) -> Result<Vec<Outcome>, String> {
    let cache_dir = workspace.join(".tinybuild").join("cache");
    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;

    let subset = match target {
        Some(t) => graph.closure(t)?,
        None => graph.tasks.keys().cloned().collect(),
    };
    let waves = graph.waves(&subset)?;
    let cache_dir = cache_dir.as_path();

    let keys: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());
    let mut outcomes = Vec::new();

    for wave in waves {
        let results: Vec<Result<(String, String, bool), String>> = std::thread::scope(|scope| {
            let handles: Vec<_> = wave
                .iter()
                .map(|name| {
                    let keys = &keys;
                    scope.spawn(move || run_task(graph, name, keys, workspace, cache_dir))
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });

        let mut guard = keys.lock().unwrap();
        for result in results {
            let (name, key, hit) = result?;
            guard.insert(name, key);
            outcomes.push(Outcome { hit });
        }
    }

    Ok(outcomes)
}

fn run_task(
    graph: &Graph,
    name: &str,
    keys: &Mutex<HashMap<String, String>>,
    workspace: &Path,
    cache_dir: &Path,
) -> Result<(String, String, bool), String> {
    let task = &graph.tasks[name];

    let dep_keys: Vec<(String, String)> = {
        let guard = keys.lock().unwrap();
        graph
            .dependencies(name)
            .iter()
            .map(|dep| (dep.clone(), guard.get(dep).cloned().unwrap_or_default()))
            .collect()
    };

    let key = cache::compute_key(task, &dep_keys, workspace)?;

    if cache::is_cached(cache_dir, &key) {
        cache::restore(cache_dir, &key, task, workspace)?;
        println!("  CACHED  {name}");
        return Ok((name.to_string(), key, true));
    }

    println!("  RUN     {name}");
    execute(task, workspace)?;
    cache::store(cache_dir, &key, task, workspace)?;
    Ok((name.to_string(), key, false))
}

fn execute(task: &Task, workspace: &Path) -> Result<(), String> {
    let status = Command::new("bash")
        .arg(&task.path)
        .current_dir(workspace)
        .status()
        .map_err(|e| format!("failed to start `{}`: {e}", task.name))?;

    if !status.success() {
        return Err(format!("task `{}` failed with {status}", task.name));
    }
    Ok(())
}
