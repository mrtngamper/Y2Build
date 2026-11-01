//! Re-pin the Alpine packages of Y2Linux's tools/build/inputs.lock.json that Alpine's mirror no longer
//! serves. Alpine keeps only the newest revision of a package in a release branch, so a locked revision
//! that received an update disappears and the build cannot fetch it. apk resolves the same package set
//! again in a scratch copy of the locked minirootfs, and every package that changed is pinned anew. The
//! refreshed lock is a source change in the working tree, to review and commit.

use crate::step::CACHE;
use crate::util::{json, sha256};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use xshell::{Shell, cmd};

/// The lock in the Y2Linux working tree.
const LOCK: &str = "/y2/Y2Build/Y2Linux/tools/build/inputs.lock.json";
/// The Alpine release branch the lock follows.
const BRANCH: &str = "https://dl-cdn.alpinelinux.org/alpine/v3.24";
const REPOSITORIES: [&str; 2] = ["main", "community"];
/// Read Y2Linux's own helpers for unpacking the minirootfs and listing what it has installed.
const PREPARE: &str = "import json, sys; from pathlib import Path; sys.path.insert(0, '/y2/Y2Build/Y2Linux'); \
                       from tools.build.prepare import extract, installed";

/// Refresh the lock when a locked package is neither cached nor served any more.
///
/// # Arguments
///
/// * `sh` - the shell to run curl, Python and apk in
///
/// # Errors
///
/// Fails if the mirror cannot be reached or apk cannot resolve the packages.
pub fn refresh_lock(sh: &Shell) -> Result<()> {
    let mut lock = json(LOCK)?;
    let packages = lock["apk_packages"].as_array().context("apk_packages")?;
    let missing: Vec<&str> = packages.iter().filter_map(|p| p["name"].as_str().zip(p["url"].as_str()))
        .filter(|(name, url)| !Path::new(&format!("{CACHE}/apks/{name}")).exists() && !served(sh, url))
        .map(|(name, _)| name).collect();
    if missing.is_empty() {
        return Ok(());
    }
    println!("Alpine no longer serves {}: refreshing Y2Linux's tools/build/inputs.lock.json", missing.join(", "));
    let names = package_names(&lock)?;
    let (inventory, minirootfs) = resolve(sh, &lock, &names)?;
    let wanted: BTreeMap<String, String> = inventory.iter().filter(|e| !minirootfs.contains(*e))
        .filter_map(|e| e.split_once('=').map(|(n, v)| (n.to_string(), v.to_string()))).collect();
    let pinned: BTreeMap<&str, &Value> = packages.iter()
        .filter_map(|p| Some((names.get(p["name"].as_str()?)?.as_str(), p))).collect();
    let mut order: Vec<&str> = packages.iter().filter_map(|p| names.get(p["name"].as_str()?).map(String::as_str))
        .filter(|n| wanted.contains_key(*n)).collect();
    order.extend(wanted.keys().map(String::as_str).filter(|n| !pinned.contains_key(n)));
    let mut refreshed = Vec::new();
    for package in order {
        let file = format!("{package}-{}.apk", wanted[package]);
        match pinned.get(package) {
            Some(old) if old["name"] == file.as_str() => refreshed.push((*old).clone()),
            old => {
                println!("  {} -> {file}", old.and_then(|o| o["name"].as_str()).unwrap_or("(new)"));
                refreshed.push(pin(sh, &file)?);
            }
        }
    }
    for gone in pinned.keys().filter(|p| !wanted.contains_key(**p)) {
        println!("  {gone}: no longer needed");
    }
    lock["apk_packages"] = json!(refreshed);
    lock["installed_packages"] = json!(inventory);
    std::fs::write(LOCK, serde_json::to_string_pretty(&lock)? + "\n")?;
    println!("Alpine lock refreshed: review and commit Y2Linux's tools/build/inputs.lock.json");
    Ok(())
}

/// Ask the mirror whether it still serves a file.
///
/// # Arguments
///
/// * `sh` - the shell to run curl in
/// * `url` - the file
fn served(sh: &Shell, url: &str) -> bool {
    cmd!(sh, "curl --silent --fail --head --location --max-time 60 {url}").quiet().ignore_stdout().run().is_ok()
}

/// Map each locked archive to the package it installs, from the lock's inventory (`name=version`).
///
/// # Arguments
///
/// * `lock` - the lock
///
/// # Returns
///
/// Each archive's file name with its package.
///
/// # Errors
///
/// Fails if an archive has no inventory entry.
fn package_names(lock: &Value) -> Result<BTreeMap<String, String>> {
    let versions: Vec<(&str, &str)> = lock["installed_packages"].as_array().into_iter().flatten()
        .filter_map(|e| e.as_str()?.split_once('=')).collect();
    lock["apk_packages"].as_array().into_iter().flatten().filter_map(|p| p["name"].as_str()).map(|file| {
        let package = versions.iter().find(|(n, v)| file == format!("{n}-{v}.apk")).with_context(|| format!("no inventory entry for {file}"))?;
        Ok((file.to_string(), package.0.to_string()))
    }).collect()
}

/// Let apk install today's revisions of the locked packages in a scratch copy of the locked minirootfs.
///
/// # Arguments
///
/// * `sh` - the shell to run Python and apk in
/// * `lock` - the lock
/// * `names` - each archive's package
///
/// # Returns
///
/// The installed inventory, and the minirootfs's own.
///
/// # Errors
///
/// Fails if the minirootfs cannot be fetched or apk fails.
fn resolve(sh: &Shell, lock: &Value, names: &BTreeMap<String, String>) -> Result<(Vec<String>, BTreeSet<String>)> {
    let base = lock["base_archives"].as_array().into_iter().flatten().find(|a| a["destination"] == "rootfs").context("minirootfs")?;
    let archive = format!("{CACHE}/downloads/{}", base["name"].as_str().unwrap_or_default());
    if !Path::new(&archive).exists() {
        download(sh, base["url"].as_str().unwrap_or_default(), &archive)?;
    }
    if sha256(std::fs::read(&archive)?) != base["sha256"].as_str().unwrap_or_default() {
        bail!("minirootfs checksum mismatch: {archive}");
    }
    let scratch = format!("{CACHE}/alpine-resolve");
    let root = format!("{scratch}/root");
    sh.remove_path(&scratch)?;
    sh.create_dir(&root)?;
    let unpack = format!("{PREPARE}; extract(Path('{archive}'), Path('{root}'))");
    cmd!(sh, "python3 -c {unpack}").quiet().run()?;
    let minirootfs: BTreeSet<String> = installed(sh, &root)?.into_iter().collect();
    sh.write_file(format!("{root}/etc/apk/repositories"), REPOSITORIES.map(|r| format!("{BRANCH}/{r}\n")).concat())?;
    let packages: BTreeSet<&String> = names.values().collect();
    let sandbox = ["--unshare-all", "--share-net", "--uid", "0", "--gid", "0", "--die-with-parent"];
    let system = ["--proc", "/proc", "--dev", "/dev", "--ro-bind", "/etc/resolv.conf", "/etc/resolv.conf"];
    cmd!(sh, "bwrap {sandbox...} --bind {root} / {system...} -- /sbin/apk add --upgrade --no-cache --quiet {packages...}").run()?;
    let inventory = installed(sh, &root)?;
    sh.remove_path(&scratch)?;
    Ok((inventory, minirootfs))
}

/// List the packages a root filesystem has installed, with Y2Linux's own reader.
///
/// # Arguments
///
/// * `sh` - the shell to run Python in
/// * `root` - the root filesystem
///
/// # Returns
///
/// Its inventory, `name=version` per package.
///
/// # Errors
///
/// Fails if the inventory cannot be read.
fn installed(sh: &Shell, root: &str) -> Result<Vec<String>> {
    let code = format!("{PREPARE}; print(json.dumps(installed(Path('{root}'))))");
    Ok(serde_json::from_str(&cmd!(sh, "python3 -c {code}").quiet().read()?)?)
}

/// Download a package archive from the mirror into the cache and pin it.
///
/// # Arguments
///
/// * `sh` - the shell to run curl in
/// * `file` - the archive's file name
///
/// # Returns
///
/// Its lock entry: name, URL and SHA-256.
///
/// # Errors
///
/// Fails if no repository serves it or the download fails.
fn pin(sh: &Shell, file: &str) -> Result<Value> {
    let url = REPOSITORIES.iter().map(|r| format!("{BRANCH}/{r}/x86_64/{file}")).find(|u| served(sh, u))
        .with_context(|| format!("{BRANCH} does not serve {file}"))?;
    let target = format!("{CACHE}/apks/{file}");
    download(sh, &url, &target)?;
    Ok(json!({"name": file, "url": url, "sha256": sha256(std::fs::read(&target)?)}))
}

/// Download a file, keeping nothing of a failed attempt.
///
/// # Arguments
///
/// * `sh` - the shell to run curl in
/// * `url` - the file
/// * `target` - where it goes
///
/// # Errors
///
/// Fails if the download fails.
fn download(sh: &Shell, url: &str, target: &str) -> Result<()> {
    let partial = format!("{target}.partial");
    sh.create_dir(Path::new(target).parent().context("download folder")?)?;
    cmd!(sh, "curl --silent --show-error --fail --location --max-time 300 --output {partial} {url}").quiet().run()?;
    std::fs::rename(partial, target)?;
    Ok(())
}
