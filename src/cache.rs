//! Content-addressed caching.
//!
//! The identity of a task is a hash of everything that can change its result:
//! the script itself, the contents of its declared inputs, the values of its
//! declared environment variables, and the identities of the tasks it depends
//! on. Same identity means same result, so we can skip the work and restore the
//! outputs we stored last time.
//!
//! Everything the task reads has to be *declared* for this to be honest. A task
//! that reads a file it did not list as an input will get the same key for two
//! different states of the world, and the cache will serve a stale result. That
//! gap between "what was declared" and "what was read" is exactly what
//! sandboxed build systems close by making undeclared reads fail.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::task::Task;

/// Compute the cache key for a task given the keys of its dependencies.
pub fn compute_key(
    task: &Task,
    dep_keys: &[(String, String)],
    workspace: &Path,
) -> Result<String, String> {
    let mut hasher = Sha256::new();

    hasher.update(b"script\0");
    hasher.update(&task.source);

    // Upstream identities. If a dependency's output changed, our key changes too.
    let mut deps: Vec<&(String, String)> = dep_keys.iter().collect();
    deps.sort();
    for (name, key) in deps {
        hasher.update(b"dep\0");
        hasher.update(name.as_bytes());
        hasher.update(key.as_bytes());
    }

    // Input files, hashed by relative path + contents, in a stable order.
    let mut matched = Vec::new();
    for pattern in &task.inputs {
        let joined = workspace.join(pattern);
        let pattern = joined.to_string_lossy();
        let paths = glob::glob(&pattern).map_err(|e| format!("bad input glob `{pattern}`: {e}"))?;
        for entry in paths {
            let path = entry.map_err(|e| e.to_string())?;
            if path.is_file() {
                matched.push(path);
            }
        }
    }
    matched.sort();
    matched.dedup();
    for path in matched {
        let rel = path.strip_prefix(workspace).unwrap_or(&path);
        let bytes =
            fs::read(&path).map_err(|e| format!("cannot read input {}: {e}", path.display()))?;
        hasher.update(b"input\0");
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(&bytes);
    }

    // Declared environment values.
    let mut env = task.env.clone();
    env.sort();
    for name in env {
        let value = std::env::var(&name).unwrap_or_default();
        hasher.update(b"env\0");
        hasher.update(name.as_bytes());
        hasher.update(b"=");
        hasher.update(value.as_bytes());
    }

    Ok(hex(hasher.finalize().as_slice()))
}

fn entry_dir(cache_dir: &Path, key: &str) -> PathBuf {
    cache_dir.join(key)
}

pub fn is_cached(cache_dir: &Path, key: &str) -> bool {
    entry_dir(cache_dir, key).join(".done").exists()
}

/// Copy the task's declared outputs into the cache entry for `key`.
pub fn store(cache_dir: &Path, key: &str, task: &Task, workspace: &Path) -> Result<(), String> {
    let dir = entry_dir(cache_dir, key);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for output in &task.outputs {
        let src = workspace.join(output);
        if src.exists() {
            copy_into(&src, &dir.join(output))?;
        }
    }
    fs::write(dir.join(".done"), b"").map_err(|e| e.to_string())?;
    Ok(())
}

/// Restore a cache entry's outputs back into the workspace.
pub fn restore(cache_dir: &Path, key: &str, task: &Task, workspace: &Path) -> Result<(), String> {
    let dir = entry_dir(cache_dir, key);
    for output in &task.outputs {
        let cached = dir.join(output);
        if cached.exists() {
            copy_into(&cached, &workspace.join(output))?;
        }
    }
    Ok(())
}

fn copy_into(src: &Path, dst: &Path) -> Result<(), String> {
    if src.is_dir() {
        fs::create_dir_all(dst).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            copy_into(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(src, dst).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
