//! Make the first-install package from the images and the stock firmware.

use crate::step::{Ctx, OUT, Step};
use anyhow::Result;
use xshell::cmd;

/// The first-install package in out/y2-first-install.
pub const STEP: Step = Step {
    name: "package",
    needs: &["artifacts"],
    after: &[],
    inputs: &["Y2Linux:tools/production"],
    settings: &["PUBLIC", "OWNER_KEY", "STOCK", "FAST"],
    source: include_str!("package.rs"),
    run,
};

/// Package the images with your SSH key (none for the public variant) and check the package with
/// Y2Linux's release checks, except with `--fast`.
///
/// # Arguments
///
/// * `c` - the running step
///
/// # Errors
///
/// Fails if packaging or a check fails.
fn run(c: &Ctx) -> Result<()> {
    let sh = &c.sh;
    sh.remove_path("out/y2-first-install")?;
    sh.remove_path(format!("{OUT}/data-seed-y2-first-install"))?;
    sh.write_file("/tmp/owner.pub", std::env::var("OWNER_KEY").unwrap_or_default() + "\n")?;
    let public = c.setting("PUBLIC").then_some("--public");
    cmd!(sh, "python3 tools/production/package.py --build out/y2-build --output out/y2-first-install --stock /stock --public-key /tmp/owner.pub {public...}").run()?;
    if !c.setting("FAST") {
        cmd!(sh, "python3 tools/production/validate.py out/y2-first-install --build out/y2-build").run()?;
    }
    Ok(())
}
