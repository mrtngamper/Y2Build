//! Fetch the locked downloads, the Alpine build userspace and the audited Linux source.

use crate::step::{Ctx, Step};
use crate::util::sha256;
use anyhow::Result;
use xshell::cmd;

/// The downloads and sources every other step builds from.
pub const STEP: Step = Step {
    name: "sources",
    needs: &[],
    after: &[],
    inputs: &[
        "Y2Linux:tools/build/inputs.lock.json", "Y2Linux:tools/build/connectivity-host.lock.json",
        "Y2Linux:tools/build/prepare.py", "Y2Linux:tools/production/prepare.py",
        "Y2Linux:docs/knowledge/linux-6.18-sources.tsv", "Y2Linux:buildroot/inputs.lock.json",
    ],
    settings: &[],
    source: include_str!("sources.rs"),
    run,
};

/// Prepare the sources, starting the build userspace over when its lock changed, since
/// tools/build/prepare.py refuses to reuse one.
///
/// # Arguments
///
/// * `c` - the running step
///
/// # Errors
///
/// Fails if a download or check fails.
fn run(c: &Ctx) -> Result<()> {
    let sh = &c.sh;
    let lock = sha256(sh.read_binary_file("tools/build/inputs.lock.json")?);
    if sh.read_file(".cache/environment/.y2-build-lock").unwrap_or_default().trim() != lock {
        sh.remove_path(".cache/environment")?;
    }
    cmd!(sh, "python3 tools/build/prepare.py").run()?;
    cmd!(sh, "python3 tools/production/prepare.py").run()?;
    Ok(())
}
