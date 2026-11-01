//! Make the update package `y2b push system` installs, in the player's own updater format (Y2Linux's
//! tools/update/package.py), signed with your push key and allowed to reinstall the same release.

use crate::step::{Ctx, OUT, Step};
use crate::trust::{PRIVATE_KEY, key_id};
use crate::util::{ensure, json};
use anyhow::{Context, Result};
use std::path::Path;
use xshell::cmd;

/// The signed update package in out/y2-ota.
pub const STEP: Step = Step {
    name: "ota",
    needs: &["artifacts"],
    after: &[],
    inputs: &["Y2Linux:tools/update/package.py"],
    settings: &["PUSH_KEY"],
    source: include_str!("ota.rs"),
    run,
};

/// Package and sign the root, boot and recovery images.
///
/// # Arguments
///
/// * `c` - the running step
///
/// # Errors
///
/// Fails if there is no push key or packaging fails.
fn run(c: &Ctx) -> Result<()> {
    let sh = &c.sh;
    ensure(Path::new(PRIVATE_KEY).is_file(), "no push key in ~/.local/share/y2build/signing (just build makes it)")?;
    let key = key_id()?;
    let sequence = json(format!("{OUT}/versions.json"))?["update_sequence"].as_i64().context("update sequence")?.to_string();
    let (image, boot, recovery, versions) = (format!("{OUT}/buildroot/images/rootfs.ext4"), format!("{OUT}/BOOTIMG.img"),
                                              format!("{OUT}/RECOVERY.img"), format!("{OUT}/versions.json"));
    sh.remove_path("out/y2-ota")?;
    cmd!(sh, "python3 tools/update/package.py package --image {image} --bootimg {boot} --recovery {recovery} --versions {versions} --output out/y2-ota --private {PRIVATE_KEY} --key-id {key} --sequence {sequence} --development-downgrade --notes 'just push'").run()?;
    Ok(())
}
