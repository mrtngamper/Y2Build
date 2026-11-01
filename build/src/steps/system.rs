//! Build Buildroot's packages that do not come from this checkout. A changed setting rebuilds them all
//! (with ccache), a changed recipe or patch rebuilds that package and the packages built on it.

use crate::buildroot;
use crate::step::{Ctx, HOST_CURRENT, OUT, SYSTEM_REBUILT, Step};
use crate::util::ensure;
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::Path;
use xshell::cmd;

/// Buildroot configured, with every package not built from this checkout.
pub const STEP: Step = Step {
    name: "system",
    needs: &["sources"],
    after: &["kernel"],
    inputs: &[
        "Y2Linux:buildroot/Config.in", "Y2Linux:buildroot/external.desc", "Y2Linux:buildroot/external.mk",
        "Y2Linux:buildroot/configs", "Y2Linux:buildroot/board/y2/busybox.fragment",
        "Y2Linux:buildroot/inputs.lock.json", "Y2Linux:tools/production/modernize_buildroot.py",
        "Y2Linux:tools/production/ffmpeg9.py", "Y2Reborn:FFMPEG_VERSION",
        "Y2Linux:buildroot/package/*", "Y2Linux:buildroot/patches/*", "Y2Linux:tools/usb",
    ],
    settings: &["PUBLIC"],
    source: include_str!("system.rs"),
    run,
};

/// Prepare Buildroot's source and configuration, start over what changed and build the packages.
///
/// # Arguments
///
/// * `c` - the running step
///
/// # Errors
///
/// Fails if preparing, configuring or building fails.
fn run(c: &Ctx) -> Result<()> {
    let global = c.changed_except(&["buildroot/package/", "buildroot/patches/", "tools/usb"]);
    let start_over = c.first || !global.is_empty();
    if start_over {
        println!("rebuilding Buildroot from scratch: {}", if c.first { "first build".into() } else { global.join(" ") });
        c.sh.remove_path(&c.buildroot)?;
        c.sh.remove_path(format!("{OUT}/buildroot"))?;
    }
    prepare_source(c)?;
    c.sh.remove_path(HOST_CURRENT)?;
    buildroot::forget_package_makefiles(c)?;
    configure(c)?;
    let rebuilt = if start_over { vec!["ALL".to_string()] } else { start_over_changed_packages(c)? };
    c.sh.write_file(SYSTEM_REBUILT, rebuilt.join("\n"))?;
    c.br().arg(format!("-j{}", c.jobs)).args(c.non_local_packages()?).run()?;
    buildroot::list_package_makefiles(c)
}

/// Extract Buildroot's source afresh with Y2Linux's updates when it is missing or its patches changed,
/// then patch it for speed.
///
/// # Arguments
///
/// * `c` - the running step
///
/// # Errors
///
/// Fails if extracting or patching fails.
fn prepare_source(c: &Ctx) -> Result<()> {
    let (sh, br) = (&c.sh, &c.buildroot);
    if !Path::new(br).is_dir() || c.changed(&["buildroot/patches/"]) {
        sh.remove_path(br)?;
        cmd!(sh, "python3 tools/production/prepare.py").ignore_stdout().run()?;
        cmd!(sh, "python3 tools/production/modernize_buildroot.py {br}").run()?;
        cmd!(sh, "python3 tools/production/ffmpeg9.py {br} /y2/Y2Linux/.cache/buildroot-dl").run()?;
    }
    buildroot::patch(br)
}

/// Configure Buildroot, without aptX for the public variant, and record the codec variant.
///
/// # Arguments
///
/// * `c` - the running step
///
/// # Errors
///
/// Fails if configuring fails or aptX stays configured in the public variant.
fn configure(c: &Ctx) -> Result<()> {
    let (sh, br) = (&c.sh, &c.buildroot);
    c.br().arg("y2_production_defconfig").run()?;
    let public = c.setting("PUBLIC");
    if public {
        cmd!(sh, "{br}/utils/config --file {OUT}/buildroot/.config --disable BR2_PACKAGE_Y2_CODEC_APTX --disable BR2_PACKAGE_LIBFREEAPTX").run()?;
        c.br().arg("olddefconfig").run()?;
        let config = sh.read_file(format!("{OUT}/buildroot/.config"))?;
        ensure(!config.lines().any(|l| l == "BR2_PACKAGE_LIBFREEAPTX=y" || l == "BR2_PACKAGE_Y2_CODEC_APTX=y"),
               "public build: aptX still configured")?;
    }
    sh.write_file(format!("{OUT}/codec-variant"), if public { "public\n" } else { "private\n" })?;
    Ok(())
}

/// Start over every package whose recipe or patches changed, and the packages built on it.
///
/// # Arguments
///
/// * `c` - the running step
///
/// # Returns
///
/// The packages started over.
///
/// # Errors
///
/// Fails if make fails.
fn start_over_changed_packages(c: &Ctx) -> Result<Vec<String>> {
    let mut stale: BTreeSet<String> = c.changes().iter()
        .filter(|i| i.starts_with("buildroot/package/") || i.starts_with("buildroot/patches/"))
        .filter_map(|i| i.rsplit('/').next().map(String::from))
        .collect();
    if c.changed(&["tools/usb"]) {
        stale.insert("y2-mtp".into());
    }
    if stale.is_empty() {
        return Ok(Vec::new());
    }
    let rdepends: Vec<String> = stale.iter().map(|p| format!("{p}-show-recursive-rdepends")).collect();
    stale.extend(c.br().arg("-s").args(&rdepends).quiet().read()?.split_whitespace().map(String::from));
    println!("starting over: {}", stale.iter().cloned().collect::<Vec<_>>().join(" "));
    let clean: Vec<String> = stale.iter()
        .filter(|p| Path::new(OUT).join("buildroot/per-package").join(p).is_dir())
        .map(|p| format!("{p}-dirclean"))
        .collect();
    if !clean.is_empty() {
        c.br().args(&clean).ignore_stdout().run()?;
    }
    Ok(stale.into_iter().collect())
}
