//! Build Linux from kernel/ and initramfs/ in Y2Linux's own sandbox (tools/build/run.py).

use crate::step::{Ctx, OUT, Step};
use crate::util::{files, sha256};
use anyhow::Result;
use std::path::Path;
use xshell::cmd;

/// The kernel and its modules, built incrementally in out/y2-build/kernel.
pub const STEP: Step = Step {
    name: "kernel",
    needs: &["sources"],
    after: &[],
    inputs: &["Y2Linux:kernel", "Y2Linux:initramfs", "Y2Linux:tools/build"],
    settings: &[],
    source: include_str!("kernel.rs"),
    run,
};

/// Configure the kernel (only when its configuration inputs changed), build it and check the
/// observer.
///
/// # Arguments
///
/// * `c` - the running step
///
/// # Errors
///
/// Fails if the configuration check or the build fails.
fn run(c: &Ctx) -> Result<()> {
    let sh = &c.sh;
    let marker = format!("{OUT}/kernel/.y2b-config");
    let config = configuration_inputs(c)?;
    let configure = if sh.read_file(&marker).unwrap_or_default() == config {
        ""
    } else {
        "make -C /src O=/build/kernel KCONFIG_ALLCONFIG=/project/kernel/config/production.config allnoconfig && \
         python3 /project/tools/build/check_config.py /build/kernel/.config /project/kernel/config/production.config &&"
    };
    sh.remove_path(&marker)?;
    let script = format!(
        "{configure} make -C /src O=/build/kernel -j{} Image zImage modules && sh /project/tools/build/dev_observer.sh",
        c.jobs);
    cmd!(sh, "python3 tools/build/run.py --output out/y2-build -- sh -c {script}").run()?;
    sh.write_file(&marker, config)?;
    Ok(())
}

/// Hash what the kernel configuration is made from: the configuration file, the patch manifest, the
/// locked Linux source and every Kconfig file.
///
/// # Arguments
///
/// * `c` - the running step
///
/// # Returns
///
/// The hash.
///
/// # Errors
///
/// Fails if a file cannot be read.
fn configuration_inputs(c: &Ctx) -> Result<String> {
    let mut bytes = Vec::new();
    for file in ["kernel/config/production.config", "kernel/patches/manifest.json", "tools/build/inputs.lock.json"] {
        bytes.extend(c.sh.read_binary_file(file)?);
    }
    for kconfig in files(Path::new("kernel")).iter().filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("Kconfig"))) {
        bytes.extend(c.sh.read_binary_file(kconfig)?);
    }
    Ok(sha256(bytes))
}
