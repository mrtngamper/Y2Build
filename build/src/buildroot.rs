//! Speed Buildroot up by patching its copy in the cache, never Y2Linux. With `Y2B_HOST_CURRENT` set it
//! skips copying every package's host folder into host/ (about 12 s). With a list in
//! `Y2B_PACKAGE_MKS_FILE` it reads only those package makefiles instead of all of them (about 6 of the
//! 7 s every make run spends reading).

use crate::step::{Ctx, OUT};
use crate::util::{ensure, files};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The list of package makefiles Buildroot reads when it exists.
const LIST: &str = "/y2/Y2Linux/out/y2-build/buildroot/.y2b-package-mks";

/// Patch Buildroot's Makefile with both shortcuts, once per extracted source.
///
/// # Arguments
///
/// * `buildroot` - Buildroot's source
///
/// # Errors
///
/// Fails if the Makefile cannot be read or written, or no longer has the lines the patch expects.
pub fn patch(buildroot: &str) -> Result<()> {
    let path = Path::new(buildroot).join("Makefile");
    let mut text = std::fs::read_to_string(&path)?;
    if !text.contains("Y2B_HOST_CURRENT") {
        let head = "host-finalize: $(PACKAGES) $(HOST_DIR) $(HOST_DIR_SYMLINK)\n";
        let tail = "\n.PHONY: staging-finalize\n";
        ensure(text.matches(head).count() == 1 && text.matches(tail).count() == 1, "Buildroot's host-finalize changed")?;
        text = text.replace(head, &format!(
            "{head}ifneq ($(Y2B_HOST_CURRENT),)\n\t@$(call MESSAGE,\"Host directory is current (y2b)\")\nelse\n"));
        text = text.replace(tail, &format!("endif\n{tail}"));
    }
    if !text.contains("Y2B_PACKAGE_MKS_FILE") {
        let line = "include $(sort $(wildcard package/*/*.mk))\n";
        ensure(text.matches(line).count() == 1, "Buildroot's package includes changed")?;
        text = text.replace(line, "Y2B_PACKAGE_MKS_FILE ?= $(BASE_DIR)/.y2b-package-mks\n\
            Y2B_PACKAGE_MKS = $(if $(wildcard $(Y2B_PACKAGE_MKS_FILE)),$(file < $(Y2B_PACKAGE_MKS_FILE)))\n\
            include $(or $(Y2B_PACKAGE_MKS),$(sort $(wildcard package/*/*.mk)))\n");
    }
    std::fs::write(&path, text)?;
    Ok(())
}

/// Remove the makefile list, so Buildroot reads every package makefile again.
///
/// # Arguments
///
/// * `c` - the running step
///
/// # Errors
///
/// Fails if the list cannot be removed.
pub fn forget_package_makefiles(c: &Ctx) -> Result<()> {
    c.sh.remove_path(LIST)?;
    Ok(())
}

/// List the package makefiles this configuration needs, and keep the list only when Buildroot sees the
/// same packages and the same variables of every enabled package with it as without it.
///
/// # Arguments
///
/// * `c` - the running step, after Buildroot is configured
///
/// # Errors
///
/// Fails if make fails or the list cannot be written.
pub fn list_package_makefiles(c: &Ctx) -> Result<()> {
    let buildroot = PathBuf::from(&c.buildroot);
    let info = c.br().args(["-s", "show-info"]).quiet().read()?;
    let enabled: Vec<String> = serde_json::from_str::<Value>(&info)?.as_object().context("show-info")?.keys().cloned().collect();
    let known = makefiles(&buildroot);
    let list = needed(&buildroot, &known, &enabled);
    let candidate = format!("{OUT}/.y2b-package-mks");
    c.sh.remove_path(format!("{candidate}.rejected"))?;
    std::fs::write(&candidate, list.join(" "))?;
    let vars = format!("VARS={}", enabled.iter().map(|n| format!("{}_%", upper(n))).collect::<Vec<_>>().join(" "));
    let short = format!("Y2B_PACKAGE_MKS_FILE={candidate}");
    let vars_all = c.br().args(["-s", "printvars", &vars]).quiet().read()?;
    let info_listed = c.br().args(["-s", "show-info", &short]).quiet().read()?;
    let vars_listed = c.br().args(["-s", "printvars", &vars, &short]).quiet().read()?;
    let differ = differences(&buildroot, &known, &enabled, &vars_all, &vars_listed);
    if info == info_listed && differ.is_empty() {
        std::fs::rename(&candidate, LIST)?;
        println!("Buildroot reads {} package makefiles instead of all", list.len());
    } else {
        std::fs::rename(&candidate, format!("{candidate}.rejected"))?;
        println!("Buildroot reads every package makefile: the short list changes what it sees ({candidate}.rejected)");
        for (name, all, listed) in differ.iter().take(5) {
            let (all, listed): (BTreeSet<&str>, BTreeSet<&str>) = (all.split(' ').collect(), listed.split(' ').collect());
            println!("  {name}: only with all {:?}, only with the list {:?}", all.difference(&listed).take(8).collect::<Vec<_>>(),
                     listed.difference(&all).take(8).collect::<Vec<_>>());
        }
    }
    Ok(())
}

/// Spell a package name the way its variables do.
///
/// # Arguments
///
/// * `name` - the package, e.g. "host-gettext-tiny"
///
/// # Returns
///
/// The prefix of its variables, e.g. "HOST_GETTEXT_TINY".
fn upper(name: &str) -> String {
    name.to_uppercase().replace(['-', '.'], "_")
}

/// Map every package to the makefile Buildroot includes for it: its own, or its parent's for a nested
/// package such as package/x11r7/xapp_xinit. A top-level package wins over a nested one of its name.
///
/// # Arguments
///
/// * `buildroot` - Buildroot's source
///
/// # Returns
///
/// Each package's variable prefix with its makefile.
fn makefiles(buildroot: &Path) -> BTreeMap<String, PathBuf> {
    let package = buildroot.join("package");
    let mut recipes: Vec<(usize, PathBuf)> = files(&package).into_iter()
        .filter(|mk| mk.extension().is_some_and(|e| e == "mk") && mk.file_stem() == mk.parent().and_then(Path::file_name))
        .map(|mk| (mk.strip_prefix(&package).map_or(0, |p| p.components().count()), mk))
        .filter(|(depth, _)| *depth == 2 || *depth == 3)
        .collect();
    recipes.sort();
    let mut found = BTreeMap::new();
    for (depth, mk) in recipes {
        let dir = mk.parent().unwrap_or(&package);
        let top = if depth == 2 {
            mk.clone()
        } else {
            let parent = dir.parent().unwrap_or(&package);
            parent.join(format!("{}.mk", parent.file_name().unwrap_or_default().to_string_lossy()))
        };
        found.entry(upper(&dir.file_name().unwrap_or_default().to_string_lossy())).or_insert(top);
    }
    found
}

/// Find the package a variable belongs to: the longest package name it starts with.
///
/// # Arguments
///
/// * `variable` - the variable
/// * `names` - the package prefixes
///
/// # Returns
///
/// The package's prefix, if any.
fn owner<'a>(variable: &str, names: &'a BTreeSet<String>) -> Option<&'a String> {
    let parts: Vec<&str> = variable.split('_').collect();
    (1..parts.len()).rev().find_map(|n| names.get(&parts[..n].join("_")))
}

/// Collect the makefiles of the enabled packages and, transitively, of every package they refer to.
///
/// # Arguments
///
/// * `buildroot` - Buildroot's source
/// * `known` - every package's makefile (see [`makefiles`])
/// * `enabled` - the configuration's packages
///
/// # Returns
///
/// The makefiles, relative to Buildroot's source, in the order Buildroot's `$(sort …)` reads them.
fn needed(buildroot: &Path, known: &BTreeMap<String, PathBuf>, enabled: &[String]) -> Vec<String> {
    let names: BTreeSet<String> = known.keys().cloned().collect();
    let mut todo: Vec<&PathBuf> = enabled.iter().filter_map(|n| known.get(&upper(n.strip_prefix("host-").unwrap_or(n)))).collect();
    let mut chosen = BTreeSet::new();
    while let Some(mk) = todo.pop() {
        if !chosen.insert(mk) {
            continue;
        }
        let text = std::fs::read_to_string(mk).unwrap_or_default();
        for reference in text.split("$(").skip(1) {
            let reference = reference.strip_prefix("HOST_").unwrap_or(reference);
            let variable: String = reference.chars().take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_').collect();
            if let Some(package) = owner(&format!("{variable}_"), &names) {
                todo.push(&known[package]);
            }
        }
    }
    let mut list: Vec<String> = chosen.iter().map(|mk| mk.strip_prefix(buildroot).unwrap_or(mk).display().to_string()).collect();
    list.sort();
    list
}

/// Compare two `printvars` outputs on the variables of the enabled packages, ignoring how many spaces
/// separate the words of a value, as make does.
///
/// # Arguments
///
/// * `buildroot` - Buildroot's source
/// * `known` - every package's makefile (see [`makefiles`])
/// * `enabled` - the configuration's packages
/// * `all`, `listed` - the outputs with every makefile and with the list
///
/// # Returns
///
/// Each differing variable with its two values.
fn differences(buildroot: &Path, known: &BTreeMap<String, PathBuf>, enabled: &[String], all: &str, listed: &str)
    -> Vec<(String, String, String)> {
    let enabled: BTreeSet<String> = enabled.iter().map(|n| upper(n)).collect();
    let toolchains = files(&buildroot.join("toolchain")).into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "mk"))
        .map(|p| upper(&p.file_stem().unwrap_or_default().to_string_lossy()));
    let names: BTreeSet<String> = known.keys().flat_map(|n| [n.clone(), format!("HOST_{n}")])
        .chain(enabled.iter().cloned()).chain(toolchains).collect();
    let ours = |text: &str| -> BTreeMap<String, String> {
        text.lines().filter_map(|line| line.split_once('='))
            .filter(|(variable, _)| owner(variable, &names).is_some_and(|o| enabled.contains(o)))
            .map(|(variable, value)| (variable.to_string(), value.split_whitespace().collect::<Vec<_>>().join(" "))).collect()
    };
    let (a, b) = (ours(all), ours(listed));
    let undefined = || "(undefined)".to_string();
    a.keys().chain(b.keys()).collect::<BTreeSet<_>>().into_iter()
        .filter(|v| a.get(*v) != b.get(*v))
        .map(|v| (v.clone(), a.get(v).cloned().unwrap_or_else(undefined), b.get(v).cloned().unwrap_or_else(undefined)))
        .collect()
}
