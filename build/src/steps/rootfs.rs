//! Assemble the root filesystem (Y2ROOT): build the marked packages of this checkout, record the
//! versions and keys, assemble the system afresh from its packages and run post-build.

use crate::step::{Ctx, HOST_CURRENT, OUT, Step};
use crate::trust::POST_BUILD;
use anyhow::{Context, Result};
use std::path::Path;
use xshell::cmd;

/// The root filesystem, rootfs.ext4 and rootfs.tar.
pub const STEP: Step = Step {
    name: "rootfs",
    needs: &["system", "reborn", "platform", "connectivity", "gpu-check", "update", "helpers"],
    after: &[],
    inputs: &[
        "Y2Linux:buildroot/board", "Y2Linux:tools/production", "Y2Linux:tools/update",
        "Y2Linux:initramfs/production", "Y2Linux:platform/capabilities.json",
        "Y2Linux:kernel/config/production.config",
    ],
    settings: &["KEYS", "COMMIT", "FAST", "PUSH_KEY"],
    source: include_str!("rootfs.rs"),
    run,
};

/// Write versions.json as build.py does: release.json and what this build is made of.
const VERSIONS: &str = r#"
import json, os, re, subprocess
from pathlib import Path
from tools.production.build import kernel_release_base
from tools.production.application import source_version
project, reborn = Path('/y2/Y2Linux'), Path('/y2/Y2Reborn')
head = lambda repo: subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip()
local = re.search(r'^CONFIG_LOCALVERSION="([^"]+)"$', (project / 'kernel/config/production.config').read_text(), re.M).group(1)
versions = {**json.loads((project / 'tools/production/release.json').read_text()), 'layout_version': 1,
            'kernel_version': kernel_release_base(project) + local, 'data_schema_version': 1,
            'reborn_version': source_version((reborn / 'Cargo.toml').read_text()),
            'build_git_commit': head(project), 'reborn_source_commit': head(reborn)}
(Path(os.environ['OUT']) / 'versions.json').write_text(json.dumps(versions, indent=2) + '\n')
"#;

/// Copy the Last.fm app key and the report upload key into the build, with build.py's own checks.
const KEYS: &str = r"
import os
from pathlib import Path
from tools.production.build import lastfm_key, report_upload_key
lastfm_key(Path(os.environ['OUT']), Path('/keys/lastfm-api.json'))
report_upload_key(Path(os.environ['OUT']), Path('/keys/report-upload.json'))
";

/// Record the versions and keys, then build the root filesystem in one make run.
///
/// # Arguments
///
/// * `c` - the running step
///
/// # Errors
///
/// Fails if a package, post-build, the images or the FFmpeg check fail.
fn run(c: &Ctx) -> Result<()> {
    let sh = &c.sh;
    c.python(VERSIONS)?;
    sh.write_file(format!("{OUT}/kernel-source-commit"), cmd!(sh, "git rev-parse HEAD").read()? + "\n")?;
    c.python(KEYS)?;
    sh.remove_path(format!("{OUT}/buildroot/target"))?;
    let host_current = Path::new(HOST_CURRENT).exists().then_some("Y2B_HOST_CURRENT=1");
    c.br()
        .arg(format!("-j{}", c.jobs))
        .arg("all")
        .args(host_current)
        .arg(format!("BR2_ROOTFS_POST_BUILD_SCRIPT={} {}", post_build_scripts(c)?, post_build_hook()?))
        .envs(c.cargo_profile())
        .run()?;
    sh.write_file(HOST_CURRENT, "")?;
    cmd!(sh, "python3 tools/production/verify-ffmpeg.py --buildroot-output {OUT}/buildroot").run()?;
    Ok(())
}

/// Read the post-build scripts Y2Linux's configuration names.
///
/// # Arguments
///
/// * `c` - the running step
///
/// # Errors
///
/// Fails if Buildroot's configuration has none.
fn post_build_scripts(c: &Ctx) -> Result<String> {
    let config = c.sh.read_file(format!("{OUT}/buildroot/.config"))?;
    let scripts = config.lines().find_map(|l| l.strip_prefix("BR2_ROOTFS_POST_BUILD_SCRIPT=")).context("post-build scripts")?;
    Ok(scripts.trim_matches('"').to_string())
}

/// Link y2b under the name Buildroot runs as the post-build script that trusts the push key.
///
/// # Returns
///
/// The link's path.
///
/// # Errors
///
/// Fails if the link cannot be made.
fn post_build_hook() -> Result<String> {
    let program = std::env::current_exe()?;
    let hook = program.with_file_name(POST_BUILD);
    if std::fs::read_link(&hook).ok().as_deref() != Some(program.as_path()) {
        let _ = std::fs::remove_file(&hook);
        std::os::unix::fs::symlink(&program, &hook)?;
    }
    Ok(hook.display().to_string())
}
