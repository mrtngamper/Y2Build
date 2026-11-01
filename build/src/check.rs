//! Run the checks before a commit on the working trees: Reborn's formatting, clippy and tests, then the
//! Y2Linux platform gate.

use anyhow::Result;
use xshell::{Shell, cmd};

/// Check Reborn and the Y2Linux platform.
///
/// # Errors
///
/// Fails if a check fails.
pub fn check() -> Result<()> {
    let sh = Shell::new()?;
    sh.change_dir("/y2/Y2Build/Y2Reborn");
    cmd!(sh, "cargo fmt --all --check").run()?;
    cmd!(sh, "cargo clippy --workspace --all-targets --locked --offline -- -D warnings").run()?;
    cmd!(sh, "cargo test --workspace --locked --offline").run()?;
    sh.change_dir("/y2/Y2Build/Y2Linux");
    cmd!(sh, "sh tools/development/platform-gate.sh").env("LD_LIBRARY_PATH", "/opt/zlib-ng/lib").run()?;
    Ok(())
}
