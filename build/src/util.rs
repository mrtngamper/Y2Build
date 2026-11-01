//! Small helpers shared by the runner, the steps and the push.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Hash bytes with SHA-256.
///
/// # Arguments
///
/// * `bytes` - the data to hash
///
/// # Returns
///
/// The hash as lowercase hex.
pub fn sha256(bytes: impl AsRef<[u8]>) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

/// Read and parse a JSON file.
///
/// # Arguments
///
/// * `path` - the file, relative to the current folder or absolute
///
/// # Returns
///
/// The parsed value.
///
/// # Errors
///
/// Fails if the file cannot be read or is not JSON.
pub fn json(path: impl AsRef<Path>) -> Result<Value> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// List every file below a folder, recursively.
///
/// # Arguments
///
/// * `dir` - the folder; a missing one has no files
///
/// # Returns
///
/// The files, sorted by path.
pub fn files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files(&path));
        } else {
            found.push(path);
        }
    }
    found.sort();
    found
}

/// Check a condition.
///
/// # Arguments
///
/// * `ok` - the condition
/// * `what` - the message if it does not hold
///
/// # Errors
///
/// Fails with `what` if the condition does not hold.
pub fn ensure(ok: bool, what: &str) -> Result<()> {
    if !ok {
        bail!("{what}");
    }
    Ok(())
}
