//! Build Reborn alone for `y2b push`: no LTO, parallel and incremental code generation, in its own
//! cargo folder, against the last build's libraries. It is not part of the image.

use crate::step::{CACHE, Ctx, OUT, Step};
use crate::util::files;
use anyhow::{Context, Result};
use std::path::Path;
use xshell::cmd;

/// reborn and libreborn_media.so, stripped, in out/reborn-quick.
pub const STEP: Step = Step {
    name: "reborn-quick",
    needs: &["system"],
    after: &[],
    inputs: &["Y2Reborn:"],
    settings: &[],
    source: include_str!("reborn_quick.rs"),
    run,
};

/// Build Reborn and strip the binary and its FFmpeg membrane into out/reborn-quick.
///
/// # Arguments
///
/// * `c` - the running step
///
/// # Errors
///
/// Fails if the build fails or the membrane is missing.
fn run(c: &Ctx) -> Result<()> {
    let sh = &c.sh;
    let host = format!("{OUT}/buildroot/per-package/reborn/host");
    let target = format!("{CACHE}/cargo/reborn-quick");
    let release = format!("{target}/armv7-unknown-linux-gnueabihf/release");
    let build_id = cmd!(sh, "git -C /y2/Y2Reborn rev-parse HEAD").read()?;
    let buildroot = format!("{OUT}/buildroot");
    cmd!(sh, "sh /y2/Y2Reborn/tools/build/cross.sh")
        .env("REBORN_BUILD_ID", build_id)
        .env("Y2_BUILDROOT_OUTPUT", buildroot)
        .env("Y2_BUILDROOT_HOST", &host)
        .env("CARGO_TARGET_DIR", &target)
        .envs([("CARGO_PROFILE_RELEASE_LTO", "false"), ("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "16"),
               ("CARGO_PROFILE_RELEASE_INCREMENTAL", "true"), ("RUSTC_WRAPPER", "")])
        .run()?;
    let media = files(Path::new(&format!("{release}/build"))).into_iter()
        .filter(|p| p.ends_with("out/libreborn_media.so"))
        .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
        .context("libreborn_media.so")?;
    let out = "/y2/Y2Linux/out/reborn-quick";
    sh.create_dir(out)?;
    cmd!(sh, "{host}/bin/arm-linux-strip -o {out}/reborn {release}/reborn").run()?;
    cmd!(sh, "{host}/bin/arm-linux-strip -o {out}/libreborn_media.so {media}").run()?;
    Ok(())
}
