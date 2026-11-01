//! Make BOOTIMG and RECOVERY: the rescue initramfs and both device trees, validated, in Y2Linux's
//! own sandbox.

use crate::step::{Ctx, Step};
use anyhow::Result;
use xshell::cmd;

/// BOOTIMG.img and RECOVERY.img in out/y2-build.
pub const STEP: Step = Step {
    name: "artifacts",
    needs: &["kernel", "rootfs", "helpers"],
    after: &[],
    inputs: &["Y2Linux:tools/build", "Y2Linux:tools/validation", "Y2Linux:tools/production",
              "Y2Linux:initramfs", "Y2Linux:kernel/dts"],
    settings: &[],
    source: include_str!("artifacts.rs"),
    run,
};

/// The commands build.py runs in the sandbox for the boot images.
const SCRIPT: &str = "cd /project && python3 -m tools.build.dev_initramfs --production && \
    Y2_DTS=innioasis-y2-production.dts sh tools/build/dtb.sh && \
    Y2_DTS=innioasis-y2-production.dts Y2_DTB_OUT=/build/y2-recovery.dtb Y2_DTS_DEFINES=-DY2_RECOVERY sh tools/build/dtb.sh && \
    python3 -m tools.validation.dev_artifacts /build --package --production";

/// Build and validate the boot images.
///
/// # Arguments
///
/// * `c` - the running step
///
/// # Errors
///
/// Fails if building or validating fails.
fn run(c: &Ctx) -> Result<()> {
    let sh = &c.sh;
    cmd!(sh, "python3 tools/build/run.py --output out/y2-build -- sh -c {SCRIPT}").run()?;
    Ok(())
}
