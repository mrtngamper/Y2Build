//! Push builds to a player over SSH: Reborn or the platform as files, or the whole system through the
//! player's own updater, which backs up the running system, installs from its recovery image, checks
//! what it wrote and goes back by itself if the new system does not start properly.

use crate::runner::{self, Options};
use crate::step::{CACHE, OUT};
use crate::trust::{key_id, with_push_key};
use anyhow::{Result, bail};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use xshell::{Shell, cmd};

/// The build's root filesystem, as Buildroot assembled it.
const TARGET: &str = "/y2/Y2Linux/out/y2-build/buildroot/target";
/// Where pushed files wait on the player before they move into place.
const STAGING: &str = "/tmp/y2b-push";
/// The platform's files besides its links, relative to the root filesystem.
const PLATFORM_FILES: [&str; 4] =
    ["usr/sbin/y2-platform", "usr/sbin/y2-platform-start", "etc/y2linux/capabilities.json", "etc/y2linux/platform-contract"];

/// Push a part of the build to the player.
///
/// # Arguments
///
/// * `args` - `reborn [--release]` (default), `platform` or `system`, with build options
///
/// # Errors
///
/// Fails if the build fails, the player cannot be reached or refuses the update.
pub fn push(args: &[String]) -> Result<()> {
    let (what, rest) = match args.split_first() {
        Some((what, rest)) if !what.starts_with('-') => (what.as_str(), rest),
        _ => ("reborn", args),
    };
    let release = rest.iter().any(|a| a == "--release");
    let options: Vec<String> = rest.iter().filter(|a| *a != "--release").cloned().collect();
    let player = Player::new()?;
    match what {
        "reborn" => push_reborn(&player, release, &options),
        "platform" => push_platform(&player, &options),
        "system" | "kernel" => push_system(&player, &options),
        other => bail!("cannot push {other:?}: y2b push [reborn [--release] | platform | system]"),
    }
}

/// Build one step and what it needs.
///
/// # Arguments
///
/// * `step` - the step
/// * `options` - build options such as `--fast`
///
/// # Errors
///
/// Fails if the build fails.
fn build(step: &str, options: &[String]) -> Result<()> {
    let mut options = Options::parse(options)?;
    options.targets = vec![step.to_string()];
    runner::build(&options)
}

/// Replace Reborn's binary and its FFmpeg membrane, then restart Reborn.
///
/// # Arguments
///
/// * `player` - the player
/// * `release` - push Reborn as the image builds it instead of the quick build
/// * `options` - build options
///
/// # Errors
///
/// Fails if the build or the push fails.
fn push_reborn(player: &Player, release: bool, options: &[String]) -> Result<()> {
    let (binary, media) = if release {
        build("rootfs", options)?;
        let target = format!("{OUT}/buildroot/per-package/reborn/target");
        (format!("{target}/usr/bin/reborn"), format!("{target}/usr/lib/reborn/libreborn_media.so"))
    } else {
        build("reborn-quick", options)?;
        ("/y2/Y2Linux/out/reborn-quick/reborn".into(), "/y2/Y2Linux/out/reborn-quick/libreborn_media.so".into())
    };
    player.install(&[(binary.into(), "usr/bin/reborn"), (media.into(), "usr/lib/reborn/libreborn_media.so")])?;
    player.run("/etc/init.d/S90reborn restart")?;
    println!("Reborn restarted on {}", player.address);
    Ok(())
}

/// Replace the platform as the build installed it (its binary, every link to it and the files
/// post-build writes for it), remove links the build no longer has and restart the player.
///
/// # Arguments
///
/// * `player` - the player
/// * `options` - build options
///
/// # Errors
///
/// Fails if the build or the push fails, or the player does not come back.
fn push_platform(player: &Player, options: &[String]) -> Result<()> {
    build("rootfs", options)?;
    let files: Vec<(PathBuf, &str)> = PLATFORM_FILES.iter().map(|f| (Path::new(TARGET).join(f), *f)).collect();
    player.install(&files)?;
    let links = platform_links()?;
    player.send(links.as_bytes(), RELINK_PLATFORM)?;
    player.restart()?;
    println!("the platform is running on {}", player.address);
    Ok(())
}

/// Remove the player's links to y2-platform that are not on stdin's list, then make the listed ones.
const RELINK_PLATFORM: &str = r#"set -e
cat > /tmp/y2b-links
for link in /bin/* /sbin/* /usr/bin/* /usr/sbin/*; do
    [ -L "$link" ] || continue
    case $(readlink "$link") in *y2-platform) ;; *) continue ;; esac
    grep -q "^${link#/} " /tmp/y2b-links || { rm "$link"; echo "removed $link"; }
done
while read -r path target; do ln -sfn "$target" "/$path"; done < /tmp/y2b-links
rm /tmp/y2b-links"#;

/// List the build's links to y2-platform.
///
/// # Returns
///
/// One "path target" line per link, paths relative to the root filesystem.
///
/// # Errors
///
/// Fails if a folder of the root filesystem cannot be read.
fn platform_links() -> Result<String> {
    let mut lines = String::new();
    for dir in ["bin", "sbin", "usr/bin", "usr/sbin"] {
        for entry in std::fs::read_dir(Path::new(TARGET).join(dir))?.flatten() {
            let Ok(target) = std::fs::read_link(entry.path()) else { continue };
            if target.to_string_lossy().ends_with("y2-platform") {
                writeln!(lines, "{dir}/{} {}", entry.file_name().to_string_lossy(), target.display())?;
            }
        }
    }
    Ok(lines)
}

/// Install the whole build through the player's updater: add the push key to its trust store once,
/// stage the signed package, apply it and wait for the player to come back.
///
/// # Arguments
///
/// * `player` - the player
/// * `options` - build options
///
/// # Errors
///
/// Fails if the build fails, the player refuses the package or does not come back.
fn push_system(player: &Player, options: &[String]) -> Result<()> {
    build("ota", options)?;
    let trust = "/etc/y2linux/update-trust.json";
    let current = player.read(&format!("cat {trust}"))?;
    if !current.contains(&format!("\"{}\"", key_id()?)) {
        println!("adding your push key to the player's trusted keys");
        player.send(with_push_key(&current)?.as_bytes(), &format!("cat > {trust}.new && mv {trust}.new {trust}"))?;
    }
    println!("copying the update to {}", player.address);
    let package: Vec<PathBuf> = std::fs::read_dir("/y2/Y2Linux/out/y2-ota")?.flatten().map(|e| e.path()).collect();
    player.run(&format!("rm -rf {STAGING} && mkdir {STAGING}"))?;
    player.copy(&package, STAGING)?;
    if player.run(&format!("y2-platform update stage --package {STAGING} >/dev/null && rm -rf {STAGING}")).is_err() {
        bail!("the player refused the update: {}", player.read("y2-platform update status")?);
    }
    println!("installing: the player restarts into its recovery, installs and restarts again");
    let _ = player.run("y2-platform update apply >/dev/null 2>&1");
    player.wait_for_restart()?;
    println!("{}", player.read("y2-platform update status")?);
    Ok(())
}

/// A player reached over SSH.
struct Player {
    sh: Shell,
    address: String,
    /// ssh's and scp's options: your key, no prompts, known hosts kept in the build state.
    options: Vec<String>,
}

impl Player {
    /// Address the player named by `Y2_PLAYER` (default root@10.42.0.1).
    ///
    /// # Errors
    ///
    /// Fails if no shell can be made.
    fn new() -> Result<Player> {
        let options = ["-i", "/ssh/key", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5",
                       "-o", "UserKnownHostsFile=/y2/state/known_hosts", "-o", "StrictHostKeyChecking=accept-new"];
        Ok(Player {
            sh: Shell::new()?,
            address: std::env::var("Y2_PLAYER").unwrap_or_else(|_| "root@10.42.0.1".into()),
            options: options.map(String::from).to_vec(),
        })
    }

    /// Run a shell script on the player.
    ///
    /// # Arguments
    ///
    /// * `script` - the script
    ///
    /// # Errors
    ///
    /// Fails if the player cannot be reached or the script fails.
    fn run(&self, script: &str) -> Result<()> {
        self.send(&[], script)
    }

    /// Run a shell script on the player and read what it prints.
    ///
    /// # Arguments
    ///
    /// * `script` - the script
    ///
    /// # Errors
    ///
    /// Fails if the player cannot be reached or the script fails.
    fn read(&self, script: &str) -> Result<String> {
        let (sh, options, address) = (&self.sh, &self.options, &self.address);
        Ok(cmd!(sh, "ssh {options...} {address} {script}").quiet().read()?)
    }

    /// Run a shell script on the player with data on its standard input.
    ///
    /// # Arguments
    ///
    /// * `input` - the data
    /// * `script` - the script
    ///
    /// # Errors
    ///
    /// Fails if the player cannot be reached or the script fails.
    fn send(&self, input: &[u8], script: &str) -> Result<()> {
        let (sh, options, address) = (&self.sh, &self.options, &self.address);
        cmd!(sh, "ssh {options...} {address} {script}").quiet().stdin(input).run()?;
        Ok(())
    }

    /// Copy files into a folder on the player.
    ///
    /// # Arguments
    ///
    /// * `files` - the files
    /// * `folder` - the folder on the player
    ///
    /// # Errors
    ///
    /// Fails if a copy fails.
    fn copy(&self, files: &[PathBuf], folder: &str) -> Result<()> {
        let (sh, options, destination) = (&self.sh, &self.options, format!("{}:{folder}/", self.address));
        cmd!(sh, "scp -q {options...} {files...} {destination}").quiet().run()?;
        Ok(())
    }

    /// Install files on the player: each is staged, then renamed into place, so a running program is
    /// replaced safely.
    ///
    /// # Arguments
    ///
    /// * `files` - each local file with its path in the player's root filesystem
    ///
    /// # Errors
    ///
    /// Fails if the player cannot be reached or a file cannot be installed.
    fn install(&self, files: &[(PathBuf, &str)]) -> Result<()> {
        let stage = Path::new(CACHE).join("push");
        self.sh.remove_path(&stage)?;
        for (local, remote) in files {
            let staged = stage.join(remote);
            self.sh.create_dir(staged.parent().unwrap_or(&stage))?;
            self.sh.copy_file(local, &staged)?;
        }
        let archive = cmd!(self.sh, "tar -C {stage} -cf - .").quiet().output()?.stdout;
        let paths: Vec<&str> = files.iter().map(|(_, remote)| *remote).collect();
        self.send(&archive, &format!(
            "set -e; rm -rf {STAGING}; mkdir {STAGING}; tar -C {STAGING} -xf -
             for path in {}; do cp -p {STAGING}/$path /$path.new; mv /$path.new /$path; done; rm -rf {STAGING}",
            paths.join(" ")))
    }

    /// Restart the player and wait until it answers again.
    ///
    /// # Errors
    ///
    /// Fails if it does not come back within five minutes.
    fn restart(&self) -> Result<()> {
        let _ = self.run("reboot");
        self.wait_for_restart()
    }

    /// Wait until the player went away and answers again.
    ///
    /// # Errors
    ///
    /// Fails if it does not come back within five minutes.
    fn wait_for_restart(&self) -> Result<()> {
        println!("waiting for {} to restart", self.address);
        let answers = || self.run("true").is_ok();
        let until = |deadline: Duration, wanted: bool| {
            let start = Instant::now();
            while start.elapsed() < deadline {
                if answers() == wanted {
                    return true;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            false
        };
        until(Duration::from_secs(60), false);
        if !until(Duration::from_secs(300), true) {
            bail!("{} did not come back within five minutes", self.address);
        }
        Ok(())
    }
}
