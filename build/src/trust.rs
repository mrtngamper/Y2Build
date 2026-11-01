//! Add your push key (made once in ~/.local/share/y2build/signing, mounted at /signing) to updater
//! trust stores as a development key, which may reinstall the same release.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::Path;
use xshell::{Shell, cmd};

/// The program name under which y2b runs as Buildroot's extra post-build script.
pub const POST_BUILD: &str = "y2b-post-build";
/// The push key's id and public half.
pub const PUSH_KEY: &str = "/signing/push.json";
/// The push key's private half.
pub const PRIVATE_KEY: &str = "/signing/push.key";

/// Make the push key once, with Y2Linux's own key generator, so later builds can sign updates.
///
/// # Arguments
///
/// * `sh` - the shell to run the generator in
///
/// # Errors
///
/// Fails if the key cannot be made or written.
pub fn ensure_key(sh: &Shell) -> Result<()> {
    if Path::new(PUSH_KEY).is_file() {
        return Ok(());
    }
    println!("making your push key in ~/.local/share/y2build/signing");
    let generated = cmd!(sh, "python3 /y2/Y2Linux/tools/update/package.py keygen --private {PRIVATE_KEY}").quiet().read()?;
    let public: Value = serde_json::from_str(&generated)?;
    let id = format!("y2build-development-{}", cmd!(sh, "date +%Y%m%d").quiet().read()?);
    let key = json!({"id": id, "public_key_hex": public["public_key_hex"]});
    std::fs::write(PUSH_KEY, serde_json::to_string_pretty(&key)? + "\n")?;
    Ok(())
}

/// Add the push key to an image's trust store. The rescue in BOOTIMG and RECOVERY copies it from there.
///
/// # Arguments
///
/// * `target` - Buildroot's target folder, as post-build scripts get it
///
/// # Errors
///
/// Fails if the trust store cannot be read, changed or written.
pub fn post_build(target: &Path) -> Result<()> {
    if !Path::new(PUSH_KEY).is_file() {
        return Ok(());
    }
    let path = target.join("etc/y2linux/update-trust.json");
    let trusted = with_push_key(&std::fs::read_to_string(&path)?)?;
    let new = path.with_extension("json.new");
    std::fs::write(&new, trusted)?;
    std::fs::rename(new, path)?;
    Ok(())
}

/// Read the push key's id.
///
/// # Errors
///
/// Fails if there is no push key.
pub fn key_id() -> Result<String> {
    let key: Value = serde_json::from_str(&std::fs::read_to_string(PUSH_KEY)?)?;
    Ok(key["id"].as_str().context("push key id")?.to_string())
}

/// Add the push key to a trust store, replacing an earlier entry with its id.
///
/// # Arguments
///
/// * `trust` - an update-trust.json
///
/// # Returns
///
/// The trust store with the key.
///
/// # Errors
///
/// Fails if the trust store or the push key cannot be parsed.
pub fn with_push_key(trust: &str) -> Result<String> {
    let mut trust: Value = serde_json::from_str(trust)?;
    let key: Value = serde_json::from_str(&std::fs::read_to_string(PUSH_KEY)?)?;
    let keys = trust["keys"].as_array_mut().context("trust store keys")?;
    keys.retain(|k| k["id"] != key["id"]);
    keys.push(json!({
        "id": key["id"], "purpose": "development", "status": "active", "public_key_hex": key["public_key_hex"],
        "minimum_sequence": 1, "maximum_sequence": 2_147_483_647, "allow_downgrade": true,
    }));
    Ok(serde_json::to_string_pretty(&trust)? + "\n")
}
