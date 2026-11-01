//! The build's steps. Their commands are those of Y2Linux's tools/production/build.py, whose version
//! the runner pins.

mod artifacts;
mod helpers;
mod kernel;
mod ota;
mod package;
mod packages;
mod reborn_quick;
mod rootfs;
mod sources;
mod system;

use crate::step::Step;

/// Every step, in reading order; the runner works out the order they run in.
pub static ALL: [&Step; 14] = [
    &sources::STEP,
    &kernel::STEP,
    &system::STEP,
    &packages::REBORN,
    &packages::PLATFORM,
    &packages::CONNECTIVITY,
    &packages::GPU_CHECK,
    &packages::UPDATE,
    &helpers::STEP,
    &rootfs::STEP,
    &artifacts::STEP,
    &package::STEP,
    &ota::STEP,
    &reborn_quick::STEP,
];
