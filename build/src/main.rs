//! Build Y2Linux and Y2Reborn incrementally in the build container and push the result to a player.
//!
//! * `y2b build [STEP…] [--public] [--fast] [--clean] [--explain] [--force STEP]`
//! * `y2b push [reborn [--release] | platform | system] [--fast] [--public]`
//! * `y2b check`, the checks before a commit
//! * `y2b step NAME`, how the runner starts a step
//! * `y2b-post-build TARGET`, how Buildroot runs y2b as a post-build script

mod buildroot;
mod check;
mod progress;
mod push;
mod runner;
mod snapshot;
mod step;
mod steps;
mod trust;
mod util;

use anyhow::{Result, bail};
use std::path::Path;

fn main() {
    if let Err(error) = run() {
        eprintln!("y2b: {error:#}");
        std::process::exit(1);
    }
}

/// Dispatch the command line to the build, the push, the checks, a single step or the post-build script.
///
/// # Errors
///
/// Fails if the command is unknown or fails.
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let program = Path::new(&args[0]).file_name().unwrap_or_default().to_string_lossy().into_owned();
    if program == trust::POST_BUILD {
        return trust::post_build(Path::new(args.get(1).map(String::as_str).unwrap_or_default()));
    }
    let rest = args.get(2..).unwrap_or_default();
    match args.get(1).map(String::as_str) {
        None | Some("build") => runner::build(&runner::Options::parse(rest)?),
        Some("push") => push::push(rest),
        Some("check") => check::check(),
        Some("step") => step::run(rest.first().map(String::as_str).unwrap_or_default()),
        Some(other) => bail!("unknown command {other:?}: y2b build | push | check | step"),
    }
}
