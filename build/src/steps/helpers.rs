//! Compile the small tools build.py puts next to the image, and make the audio fixtures.

use crate::step::{Ctx, OUT, Step};
use anyhow::Result;
use xshell::cmd;

/// y2-fbtest, y2-abi-check, y2-usb-status, y2-platform-start, y2-offline-charge and the audio fixtures.
pub const STEP: Step = Step {
    name: "helpers",
    needs: &["system"],
    after: &[],
    inputs: &[
        "Y2Linux:tools/development/fbtest.c", "Y2Linux:tools/development/abi-check.c",
        "Y2Linux:tools/production/usb-status.c", "Y2Linux:tools/production/platform-start.c",
        "Y2Linux:tools/production/offline-charge.c", "Y2Linux:tools/development/audio-fixtures.py",
    ],
    settings: &[],
    source: include_str!("helpers.rs"),
    run,
};

/// Compile the tools with Buildroot's cross compiler and make the audio fixtures.
///
/// # Arguments
///
/// * `c` - the running step
///
/// # Errors
///
/// Fails if a compile or the fixtures fail.
fn run(c: &Ctx) -> Result<()> {
    let sh = &c.sh;
    let cc = format!("{OUT}/buildroot/per-package/toolchain-external-bootlin/host/bin/arm-linux-gcc");
    let strict = ["-Os", "-Wall", "-Wextra", "-Werror"];
    cmd!(sh, "{cc} {strict...} tools/development/fbtest.c -o {OUT}/y2-fbtest").run()?;
    cmd!(sh, "{cc} {strict...} -mcpu=cortex-a7 -mfpu=neon-vfpv4 -mfloat-abi=hard -marm -pthread tools/development/abi-check.c -o {OUT}/y2-abi-check").run()?;
    for tool in ["usb-status", "platform-start", "offline-charge"] {
        cmd!(sh, "{cc} {strict...} tools/production/{tool}.c -o {OUT}/y2-{tool}").run()?;
    }
    cmd!(sh, "python3 tools/development/audio-fixtures.py {OUT}/audio").run()?;
    Ok(())
}
