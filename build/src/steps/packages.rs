//! Mark the packages built from this checkout for step `rootfs`, which rebuilds them in their kept
//! folders in the same make run as the root filesystem, so make and cargo compile only what changed.

use crate::step::Step;

/// Reborn, the player.
pub const REBORN: Step = Step {
    name: "reborn",
    needs: &["system"],
    after: &[],
    inputs: &["Y2Reborn:", "Y2Linux:buildroot/package/reborn", "Y2Linux:tools/graphics/reborn-splash.c",
              "Y2Linux:tools/production/application.py"],
    settings: &["FAST"],
    source: include_str!("packages.rs"),
    run: |c| c.mark_local_package("reborn", true),
};

/// The platform: every y2-platform command and daemon.
pub const PLATFORM: Step = Step {
    name: "platform",
    needs: &["system"],
    after: &[],
    inputs: &["Y2Linux:platform", "Y2Linux:buildroot/package/y2-platform"],
    settings: &["FAST"],
    source: include_str!("packages.rs"),
    run: |c| c.mark_local_package("y2-platform", true),
};

/// The Wi-Fi and Bluetooth tools.
pub const CONNECTIVITY: Step = Step {
    name: "connectivity",
    needs: &["system"],
    after: &[],
    inputs: &["Y2Linux:tools/connectivity", "Y2Linux:buildroot/package/y2-connectivity"],
    settings: &[],
    source: include_str!("packages.rs"),
    run: |c| c.mark_local_package("y2-connectivity", false),
};

/// The GPU check.
pub const GPU_CHECK: Step = Step {
    name: "gpu-check",
    needs: &["system"],
    after: &[],
    inputs: &["Y2Linux:tools/graphics", "Y2Linux:buildroot/package/y2-gpu-check"],
    settings: &[],
    source: include_str!("packages.rs"),
    run: |c| c.mark_local_package("y2-gpu-check", false),
};

/// The updater's native core.
pub const UPDATE: Step = Step {
    name: "update",
    needs: &["system"],
    after: &[],
    inputs: &["Y2Linux:tools/update", "Y2Linux:buildroot/package/y2-update"],
    settings: &[],
    source: include_str!("packages.rs"),
    run: |c| c.mark_local_package("y2-update", false),
};
