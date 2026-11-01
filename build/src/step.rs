//! Define a build step and the context every step runs in: the environment of Y2Linux's
//! tools/production/build.py, Buildroot's make and what changed since the step last ran.

use crate::util::json;
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use xshell::{Cmd, Shell, cmd};

/// One step of the build. The runner reruns it only when its key changes, which covers its source,
/// the shared code, the git content of its inputs, its settings and the keys of the steps it needs.
pub struct Step {
    pub name: &'static str,
    /// Steps whose output this one uses; a change in one of them reruns this step.
    pub needs: &'static [&'static str],
    /// Steps that must finish first when both run, without invalidating this one.
    pub after: &'static [&'static str],
    /// `<repo>:<path>` inputs in the build snapshots; a trailing `/*` stands for each child.
    pub inputs: &'static [&'static str],
    /// Build settings (see the runner) the output depends on.
    pub settings: &'static [&'static str],
    /// The step's own source, so editing the step reruns it.
    pub source: &'static str,
    pub run: fn(&Ctx) -> Result<()>,
}

/// Code every step uses, so editing it reruns every step.
pub const SHARED: [&str; 4] =
    [include_str!("step.rs"), include_str!("buildroot.rs"), include_str!("trust.rs"), include_str!("util.rs")];

/// The build folder.
pub const OUT: &str = "/y2/Y2Linux/out/y2-build";
/// Downloads, Buildroot's source and the compiler caches.
pub const CACHE: &str = "/y2/Y2Linux/.cache";
/// Marker that Buildroot's host/ is current, so step `rootfs` may skip copying it.
pub const HOST_CURRENT: &str = "/y2/Y2Linux/out/y2-build/.y2b-host-current";
/// What step `system` started over ("ALL" for everything), for the local packages built on it.
pub const SYSTEM_REBUILT: &str = "/y2/Y2Linux/out/y2-build/.y2b-system-rebuilt";
/// The Buildroot packages built from this checkout, which step `rootfs` builds.
pub const LOCAL_PACKAGES: [&str; 5] = ["reborn", "y2-platform", "y2-connectivity", "y2-gpu-check", "y2-update"];

/// Run one step, as the runner starts it with `y2b step NAME`.
///
/// # Arguments
///
/// * `name` - the step
///
/// # Errors
///
/// Fails if there is no such step or the step fails.
pub fn run(name: &str) -> Result<()> {
    let step = crate::steps::ALL.iter().find(|s| s.name == name).with_context(|| format!("no step {name}"))?;
    (step.run)(&Ctx::new()?)
}

/// What a running step works with.
pub struct Ctx {
    pub sh: Shell,
    /// Changed inputs (by path), needed steps (`dep:<step>`) and settings (`setting:<name>`).
    changed: Vec<String>,
    /// Whether the step never succeeded before.
    pub first: bool,
    /// How many jobs make runs at once.
    pub jobs: String,
    /// Buildroot's source in the cache.
    pub buildroot: String,
    reborn_version: String,
}

impl Ctx {
    /// Set up the environment tools/production/build.py gives every step.
    ///
    /// # Returns
    ///
    /// The context, with its shell in /y2/Y2Linux.
    ///
    /// # Errors
    ///
    /// Fails if the lock files cannot be read or Reborn's version cannot be found.
    fn new() -> Result<Ctx> {
        let sh = Shell::new()?;
        sh.change_dir("/y2/Y2Linux");
        std::env::set_current_dir("/y2/Y2Linux")?;
        let lock = json("buildroot/inputs.lock.json")?;
        let epoch = lock["source_date_epoch"].to_string();
        let build_id = build_id(&json("tools/production/release.json")?);
        for (name, value) in [
            ("OUT", OUT), ("Y2_ARTIFACT_DIR", OUT), ("BR2_DL_DIR", "/y2/Y2Linux/.cache/buildroot-dl"),
            ("PYTHONPATH", "/y2/Y2Linux"), ("PYTHONDONTWRITEBYTECODE", "1"), ("LC_ALL", "C"), ("TZ", "UTC"),
            ("SOURCE_DATE_EPOCH", &epoch), ("E2FSPROGS_FAKE_TIME", &epoch), ("Y2_BUILD_ID", &build_id),
        ] {
            sh.set_var(name, value);
        }
        sh.create_dir(OUT)?;
        let reborn_version =
            cmd!(sh, "python3 tools/production/application.py --source-version /y2/Y2Reborn/Cargo.toml").read()?;
        Ok(Ctx {
            changed: std::env::var("Y2B_CHANGED").unwrap_or_default().split_whitespace().map(String::from).collect(),
            first: std::env::var("Y2B_FIRST").is_ok_and(|v| !v.is_empty()),
            jobs: std::thread::available_parallelism().map_or(4, std::num::NonZero::get).to_string(),
            buildroot: format!("{CACHE}/sources/buildroot-{}", lock["version"].as_str().context("Buildroot version")?),
            reborn_version,
            sh,
        })
    }

    /// Tell whether a build setting is on.
    ///
    /// # Arguments
    ///
    /// * `name` - the setting, e.g. "FAST"
    pub fn setting(&self, name: &str) -> bool {
        std::env::var(name).is_ok_and(|v| !v.is_empty())
    }

    /// List what changed since the step last succeeded.
    pub fn changes(&self) -> &[String] {
        &self.changed
    }

    /// Tell whether anything starting with one of the prefixes changed.
    ///
    /// # Arguments
    ///
    /// * `prefixes` - input paths, `dep:<step>` or `setting:<name>`
    pub fn changed(&self, prefixes: &[&str]) -> bool {
        self.changed.iter().any(|c| prefixes.iter().any(|p| c.starts_with(p)))
    }

    /// List the changed inputs and settings that start with none of the prefixes.
    ///
    /// # Arguments
    ///
    /// * `prefixes` - input paths to leave out
    ///
    /// # Returns
    ///
    /// The other changes, needed steps left out.
    pub fn changed_except(&self, prefixes: &[&str]) -> Vec<String> {
        self.changed.iter()
            .filter(|c| !c.starts_with("dep:") && !prefixes.iter().any(|p| c.starts_with(p)))
            .cloned().collect()
    }

    /// Prepare Buildroot's make for the Y2Linux tree. y2-platform's build folder and Reborn's version
    /// are fixed, so make neither starts the platform over nor asks Python on every read.
    ///
    /// # Returns
    ///
    /// The command, to add targets to.
    pub fn br(&self) -> Cmd<'_> {
        let (sh, source, version) = (&self.sh, &self.buildroot, &self.reborn_version);
        cmd!(sh, "make -C {source} O={OUT}/buildroot BR2_EXTERNAL=/y2/Y2Linux/buildroot Y2_PLATFORM_VERSION=y2b REBORN_VERSION={version}")
    }

    /// List Buildroot's enabled packages that are not built from this checkout.
    ///
    /// # Errors
    ///
    /// Fails if make fails.
    pub fn non_local_packages(&self) -> Result<Vec<String>> {
        let targets = self.br().args(["-s", "show-targets"]).quiet().read()?;
        Ok(targets.split_whitespace()
            .filter(|p| !p.starts_with("rootfs-") && !LOCAL_PACKAGES.contains(p))
            .map(String::from).collect())
    }

    /// Find Buildroot's build folder of a package of this checkout.
    ///
    /// # Arguments
    ///
    /// * `package` - one of [`LOCAL_PACKAGES`]
    ///
    /// # Errors
    ///
    /// Fails if the package's recipe has no version.
    pub fn package_dir(&self, package: &str) -> Result<PathBuf> {
        let version = match package {
            "reborn" => self.reborn_version.clone(),
            "y2-platform" => "y2b".to_string(),
            _ => {
                let recipe = self.sh.read_file(format!("buildroot/package/{package}/{package}.mk"))?;
                let field = format!("{}_VERSION = ", package.to_uppercase().replace('-', "_"));
                recipe.lines().find_map(|l| l.strip_prefix(&field)).context("package version")?.trim().to_string()
            }
        };
        Ok(PathBuf::from(format!("{OUT}/buildroot/build/{package}-{version}")))
    }

    /// Mark a package of this checkout for step `rootfs` to rebuild in its kept folder, as
    /// `make <package>-rebuild` would, and start it over when step `system` rebuilt a package it is
    /// built on.
    ///
    /// # Arguments
    ///
    /// * `package` - one of [`LOCAL_PACKAGES`]
    /// * `cargo` - whether it is built with cargo, whose folder is kept in the cache
    ///
    /// # Errors
    ///
    /// Fails if a folder cannot be changed or make fails.
    pub fn mark_local_package(&self, package: &str, cargo: bool) -> Result<()> {
        let dir = self.package_dir(package)?;
        if self.built_on_rebuilt(package)? {
            println!("starting {package} over, a package it is built on was");
            self.sh.remove_path(&dir)?;
            self.sh.remove_path(format!("{OUT}/buildroot/per-package/{package}"))?;
            for mode in ["release", "fast"] {
                self.sh.remove_path(format!("{CACHE}/cargo/{package}-{mode}"))?;
            }
            self.sh.remove_path(HOST_CURRENT)?;
        }
        if cargo {
            self.link_cargo_folder(package, &dir)?;
        }
        for stamp in ["rsynced", "built", "installed", "target_installed", "staging_installed", "images_installed", "host_installed"] {
            self.sh.remove_path(dir.join(format!(".stamp_{stamp}")))?;
        }
        Ok(())
    }

    /// Tell whether step `system` just rebuilt a package this one is built on.
    ///
    /// # Arguments
    ///
    /// * `package` - one of [`LOCAL_PACKAGES`]
    ///
    /// # Errors
    ///
    /// Fails if make fails.
    fn built_on_rebuilt(&self, package: &str) -> Result<bool> {
        let rebuilt = self.sh.read_file(SYSTEM_REBUILT).unwrap_or_default();
        if rebuilt.is_empty() || !self.changed(&["dep:system"]) || !Path::new(OUT).join("buildroot/build").is_dir() {
            return Ok(false);
        }
        let rebuilt: Vec<&str> = rebuilt.split_whitespace().collect();
        let depends = self.br().args(["-s", &format!("{package}-show-recursive-depends")]).quiet().read()?;
        Ok(rebuilt.contains(&"ALL") || depends.split_whitespace().any(|d| rebuilt.contains(&d)))
    }

    /// Point a Rust package's cargo folder at one kept in the cache for the current mode, so switching
    /// `--fast` does not recompile everything. A folder from before becomes the release one.
    ///
    /// # Arguments
    ///
    /// * `package` - the package
    /// * `dir` - its build folder
    ///
    /// # Errors
    ///
    /// Fails if a folder or link cannot be made.
    fn link_cargo_folder(&self, package: &str, dir: &Path) -> Result<()> {
        let mode = if self.setting("FAST") { "fast" } else { "release" };
        let folder = format!("{CACHE}/cargo/{package}-{mode}");
        let release = format!("{CACHE}/cargo/{package}-release");
        let target = dir.join("target");
        self.sh.create_dir(dir)?;
        if target.is_dir() && !target.is_symlink() && !Path::new(&release).exists() {
            self.sh.create_dir(format!("{CACHE}/cargo"))?;
            std::fs::rename(&target, &release)?;
        }
        self.sh.remove_path(&target)?;
        self.sh.create_dir(&folder)?;
        std::os::unix::fs::symlink(&folder, &target)?;
        Ok(())
    }

    /// Give cargo's release profile for `--fast`: no LTO, parallel and incremental code generation.
    ///
    /// # Returns
    ///
    /// The environment variables to set, none without `--fast`.
    pub fn cargo_profile(&self) -> Vec<(&'static str, &'static str)> {
        if !self.setting("FAST") {
            return Vec::new();
        }
        vec![
            ("CARGO_PROFILE_RELEASE_LTO", "false"), ("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "16"),
            ("CARGO_PROFILE_RELEASE_INCREMENTAL", "true"), ("RUSTC_WRAPPER", ""),
        ]
    }

    /// Run Python code that uses Y2Linux's own modules.
    ///
    /// # Arguments
    ///
    /// * `code` - the program
    ///
    /// # Errors
    ///
    /// Fails if the program fails.
    pub fn python(&self, code: &str) -> Result<()> {
        let sh = &self.sh;
        cmd!(sh, "python3 -c {code}").quiet().run()?;
        Ok(())
    }
}

/// Make build.py's build ID.
///
/// # Arguments
///
/// * `release` - Y2Linux's tools/production/release.json
///
/// # Returns
///
/// release.json's own ID, or one made from the root filesystem's version.
fn build_id(release: &Value) -> String {
    if let Some(id) = release["build_id"].as_str() {
        return id.to_string();
    }
    let version = release["rootfs_version"].as_str().unwrap_or_default();
    format!("Y2LINUX-REBORN-PREMIUM-{:0>2}", version.rsplit('.').next().unwrap_or_default())
}
