//! The store.
//!
//! A content-addressed store, shared across projects rather than living inside
//! one repository. Outputs are kept by the hash of their bytes, so they are
//! immutable and deduplicated: the same artifact built anywhere lands at the
//! same place. This is the small version of the idea Nix takes to its limit
//! with `/nix/store`.
//!
//! On top of the blob store sits an action cache mapping a task's key to the
//! manifest of outputs it produced, an optional substituter (a remote store we
//! pull from on a miss and push to on a hit), and a garbage collector that
//! keeps only what the latest build can reach.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex(&hasher.finalize())
}

/// One file inside a declared output. `sub` is empty for a file output, or the
/// path within the directory for a directory output.
#[derive(Serialize, Deserialize, Clone)]
pub struct Leaf {
    pub sub: String,
    pub hash: String,
    pub exec: bool,
}

/// A task's outputs: declared output path -> the files it expands to.
pub type Manifest = BTreeMap<String, Vec<Leaf>>;

/// The hash of a manifest. This is a task's *output* identity, and feeding it
/// into a dependent's key is what gives us early cutoff: if a task reruns but
/// produces identical bytes, this does not change, so dependents stay cached.
pub fn manifest_hash(manifest: &Manifest) -> String {
    let bytes = serde_json::to_vec(manifest).unwrap_or_default();
    hash_bytes(&bytes)
}

pub struct Store {
    pub root: PathBuf,
    pub substituter: Option<PathBuf>,
}

impl Store {
    pub fn open(root: PathBuf, substituter: Option<PathBuf>) -> Result<Store, String> {
        fs::create_dir_all(root.join("cas")).map_err(|e| e.to_string())?;
        fs::create_dir_all(root.join("actions")).map_err(|e| e.to_string())?;
        Ok(Store { root, substituter })
    }

    fn cas_path(&self, hash: &str) -> PathBuf {
        self.root.join("cas").join(hash)
    }

    fn action_path(&self, key: &str) -> PathBuf {
        self.root.join("actions").join(format!("{key}.json"))
    }

    /// Make sure a blob is present locally, pulling from the substituter if not.
    fn ensure_blob(&self, hash: &str) -> Result<bool, String> {
        if self.cas_path(hash).exists() {
            return Ok(true);
        }
        if let Some(sub) = &self.substituter {
            let src = sub.join("cas").join(hash);
            if src.exists() {
                fs::copy(&src, self.cas_path(hash)).map_err(|e| e.to_string())?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn put_blob(&self, bytes: &[u8]) -> Result<String, String> {
        let hash = hash_bytes(bytes);
        let path = self.cas_path(&hash);
        if !path.exists() {
            fs::write(&path, bytes).map_err(|e| e.to_string())?;
        }
        if let Some(sub) = &self.substituter {
            let dst = sub.join("cas").join(&hash);
            if !dst.exists() {
                if let Some(parent) = dst.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::copy(&path, &dst);
            }
        }
        Ok(hash)
    }

    /// Store a declared output (a file or a directory) found under `base`.
    pub fn put_output(&self, base: &Path, decl: &str) -> Result<Vec<Leaf>, String> {
        let target = base.join(decl);
        let mut leaves = Vec::new();
        if target.is_dir() {
            self.collect_dir(&target, &target, &mut leaves)?;
        } else if target.is_file() {
            let bytes = fs::read(&target).map_err(|e| format!("read {}: {e}", target.display()))?;
            leaves.push(Leaf {
                sub: String::new(),
                hash: self.put_blob(&bytes)?,
                exec: is_exec(&target),
            });
        } else {
            return Err(format!("task did not produce declared output `{decl}`"));
        }
        leaves.sort_by(|a, b| a.sub.cmp(&b.sub));
        Ok(leaves)
    }

    fn collect_dir(&self, root: &Path, dir: &Path, out: &mut Vec<Leaf>) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                self.collect_dir(root, &path, out)?;
            } else {
                let bytes = fs::read(&path).map_err(|e| e.to_string())?;
                let sub = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                out.push(Leaf {
                    sub,
                    hash: self.put_blob(&bytes)?,
                    exec: is_exec(&path),
                });
            }
        }
        Ok(())
    }

    /// Materialize a declared output back into `dest_base`.
    pub fn materialize(&self, dest_base: &Path, decl: &str, leaves: &[Leaf]) -> Result<(), String> {
        for leaf in leaves {
            if !self.ensure_blob(&leaf.hash)? {
                return Err(format!("missing blob {} for output `{decl}`", leaf.hash));
            }
            let dst = if leaf.sub.is_empty() {
                dest_base.join(decl)
            } else {
                dest_base.join(decl).join(&leaf.sub)
            };
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let _ = fs::remove_file(&dst);
            fs::copy(self.cas_path(&leaf.hash), &dst).map_err(|e| e.to_string())?;
            if leaf.exec {
                set_exec(&dst);
            }
        }
        Ok(())
    }

    pub fn get_action(&self, key: &str) -> Option<Manifest> {
        if let Ok(data) = fs::read(self.action_path(key)) {
            return serde_json::from_slice(&data).ok();
        }
        if let Some(sub) = &self.substituter {
            let src = sub.join("actions").join(format!("{key}.json"));
            if let Ok(data) = fs::read(&src) {
                let _ = fs::write(self.action_path(key), &data);
                return serde_json::from_slice(&data).ok();
            }
        }
        None
    }

    pub fn put_action(&self, key: &str, manifest: &Manifest) -> Result<(), String> {
        let data = serde_json::to_vec(manifest).map_err(|e| e.to_string())?;
        fs::write(self.action_path(key), &data).map_err(|e| e.to_string())?;
        if let Some(sub) = &self.substituter {
            let dst = sub.join("actions").join(format!("{key}.json"));
            if let Some(parent) = dst.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(dst, &data);
        }
        Ok(())
    }

    pub fn sandbox_for(&self, id: &str) -> PathBuf {
        self.root.join("sandbox").join(id.replace(['/', '\\'], "_"))
    }

    /// Keep only the actions in `roots` and the blobs they reference.
    pub fn gc(&self, roots: &[String]) -> Result<(usize, usize), String> {
        let keep_keys: std::collections::HashSet<&str> = roots.iter().map(|s| s.as_str()).collect();
        let mut keep_blobs = std::collections::HashSet::new();
        for key in roots {
            if let Some(manifest) = self.get_action(key) {
                for leaves in manifest.values() {
                    for leaf in leaves {
                        keep_blobs.insert(leaf.hash.clone());
                    }
                }
            }
        }

        let mut removed_actions = 0;
        for entry in fs::read_dir(self.root.join("actions")).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            let key = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if !keep_keys.contains(key) {
                let _ = fs::remove_file(&path);
                removed_actions += 1;
            }
        }

        let mut removed_blobs = 0;
        for entry in fs::read_dir(self.root.join("cas")).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            let hash = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !keep_blobs.contains(hash) {
                let _ = fs::remove_file(&path);
                removed_blobs += 1;
            }
        }
        Ok((removed_actions, removed_blobs))
    }
}

#[cfg(unix)]
fn is_exec(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(unix)]
fn set_exec(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(perms.mode() | 0o755);
        let _ = fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn is_exec(_path: &Path) -> bool {
    false
}

#[cfg(not(unix))]
fn set_exec(_path: &Path) {}
