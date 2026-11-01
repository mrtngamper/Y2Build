//! Work out which steps a build needs and which of them changed, and run those, independent ones side
//! by side. Each step runs as `y2b step NAME` with its output in out/logs/NAME.log.

use crate::progress::{Progress, Row};
use crate::snapshot::{self, REPOS};
use crate::step::{SHARED, Step};
use crate::steps::ALL;
use crate::trust::PUSH_KEY;
use crate::util::{files, sha256};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use xshell::Shell;

/// What each step was last built from.
const STATE: &str = "/y2/state";
/// The steps' logs.
const LOGS: &str = "/y2/Y2Linux/out/logs";
/// The version of Y2Linux's tools/production/build.py whose commands the steps follow. When it
/// changes, the build stops until the steps are brought in line with it and this pin is updated.
const UPSTREAM: (&str, &str) =
    ("tools/production/build.py", "de6802f50bc287112b28876311defe2caa9f46f4dd3c5011fbd1f54b882661b3");

/// What a build is asked to do.
#[derive(Default)]
pub struct Options {
    /// Steps to build, "all" (the package) or "boot" (the images); none means "all".
    pub targets: Vec<String>,
    pub public: bool,
    pub fast: bool,
    pub clean: bool,
    pub explain: bool,
    pub force: Vec<String>,
}

impl Options {
    /// Parse the build's command line.
    ///
    /// # Arguments
    ///
    /// * `args` - targets and options
    ///
    /// # Errors
    ///
    /// Fails on an unknown option or `--force` without a step.
    pub fn parse(args: &[String]) -> Result<Options> {
        let mut options = Options::default();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--public" => options.public = true,
                "--fast" => options.fast = true,
                "--clean" => options.clean = true,
                "--explain" => options.explain = true,
                "--force" => options.force.push(args.next().context("--force STEP")?.clone()),
                flag if flag.starts_with('-') => bail!("unknown option {flag}"),
                target => options.targets.push(target.to_string()),
            }
        }
        Ok(options)
    }
}

/// A step that has to run, and why.
struct Run {
    step: &'static Step,
    key: String,
    record: Value,
    changed: Vec<String>,
    first: bool,
    resuming: bool,
    /// How long the step took last time.
    expected: Option<Duration>,
}

/// Build: refresh the Alpine lock if the mirror dropped a locked package, snapshot the working trees,
/// find the steps whose key changed and run them.
///
/// # Arguments
///
/// * `options` - what to build
///
/// # Errors
///
/// Fails if another build runs, Y2Linux's build.py changed, or a step fails.
pub fn build(options: &Options) -> Result<()> {
    let started = Instant::now();
    fs::create_dir_all(STATE)?;
    let _lock = lock()?;
    let sh = Shell::new()?;
    crate::alpine::refresh_lock(&sh)?;
    let commits = snapshots(&sh)?;
    crate::trust::ensure_key(&sh)?;
    let pinned = sha256(fs::read(Path::new("/y2/Y2Linux").join(UPSTREAM.0))?);
    if pinned != UPSTREAM.1 {
        bail!("Y2Linux's {} changed (sha256 {pinned}): bring the steps in line, then update UPSTREAM in runner.rs", UPSTREAM.0);
    }
    if options.clean {
        clean(&sh)?;
    }
    if options.fast {
        println!("--fast: Reborn and the platform without LTO, the package not checked; not for release");
    }
    let runs = runs(&sh, options, &commits)?;
    if runs.is_empty() {
        println!("up to date ({:.1} s)", started.elapsed().as_secs_f64());
        return Ok(());
    }
    for run in &runs {
        println!("  {:14} {}", run.step.name, reason(run, options));
    }
    if !options.explain {
        execute(runs, &settings(options, &commits))?;
        println!("built in {:.1} s", started.elapsed().as_secs_f64());
    }
    Ok(())
}

/// Take the snapshots of both repositories and say which hold uncommitted changes.
///
/// # Arguments
///
/// * `sh` - the shell to run git in
///
/// # Returns
///
/// Each repository with its snapshot commit.
///
/// # Errors
///
/// Fails if a snapshot cannot be taken.
fn snapshots(sh: &Shell) -> Result<Vec<(String, String)>> {
    let mut commits = Vec::new();
    for name in REPOS {
        let (commit, dirty) = snapshot::take(sh, name)?;
        println!("{name} {}{}", &commit[..12], if dirty { " (with uncommitted changes)" } else { "" });
        commits.push((name.to_string(), commit));
    }
    Ok(commits)
}

/// Forget every step and remove the build folder; downloads and compiler caches stay.
///
/// # Arguments
///
/// * `sh` - the shell to remove files with
///
/// # Errors
///
/// Fails if a file cannot be removed.
fn clean(sh: &Shell) -> Result<()> {
    for entry in fs::read_dir(STATE)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("step-") || name.starts_with("attempt-") || name.starts_with("unit-") {
            fs::remove_file(entry.path())?;
        }
    }
    sh.remove_path("/y2/Y2Linux/out/y2-build")?;
    Ok(())
}

/// Find the steps that have to run: those whose key differs from their last successful run.
///
/// # Arguments
///
/// * `sh` - the shell to run git in
/// * `options` - what to build
/// * `commits` - the snapshot commits
///
/// # Returns
///
/// The steps to run, needed ones first.
///
/// # Errors
///
/// Fails on an unknown step or if git fails.
fn runs(sh: &Shell, options: &Options, commits: &[(String, String)]) -> Result<Vec<Run>> {
    let settings = settings(options, commits);
    let shared = SHARED.concat();
    let mut keys: BTreeMap<&str, String> = BTreeMap::new();
    let mut runs = Vec::new();
    for step in plan(&options.targets)? {
        let inputs: BTreeMap<String, String> = snapshot::object_ids(sh, step.inputs, commits)?.into_iter().collect();
        let needs: BTreeMap<&str, &String> = step.needs.iter().map(|n| (*n, &keys[n])).collect();
        let own: BTreeMap<&str, &String> = step.settings.iter().map(|s| (*s, &settings[s])).collect();
        let record = json!({"inputs": inputs, "needs": needs, "settings": own});
        let key = sha256(format!("{shared}\n{}\n{record}", step.source));
        keys.insert(step.name, key.clone());
        let last = last_record(step.name);
        let forced = options.force.iter().any(|f| f == step.name);
        if !forced && last.as_ref().is_some_and(|l| l["key"] == key.as_str()) {
            continue;
        }
        let resuming = fs::read_to_string(format!("{STATE}/attempt-{}", step.name)).is_ok_and(|a| a == key);
        let changed = match &last {
            Some(last) if !resuming => changes(&record, last),
            _ => Vec::new(),
        };
        let expected = last.as_ref().and_then(|l| l["seconds"].as_f64()).map(Duration::from_secs_f64);
        runs.push(Run { step, key, record, changed, first: last.is_none() && !resuming, resuming, expected });
    }
    Ok(runs)
}

/// Read what a step was last built from, taking over the record of the earlier Python runner.
///
/// # Arguments
///
/// * `name` - the step
///
/// # Returns
///
/// The record, if the step ever succeeded.
fn last_record(name: &str) -> Option<Value> {
    let read = |path: String| fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    read(format!("{STATE}/step-{name}.json")).or_else(|| {
        let old = read(format!("{STATE}/unit-{name}.json"))?;
        Some(json!({"key": "", "inputs": old["inputs"], "needs": old["deps"], "settings": old["vars"]}))
    })
}

/// Say why a step runs.
///
/// # Arguments
///
/// * `run` - the step and its changes
/// * `options` - what the build was asked to do
///
/// # Returns
///
/// A short reason, e.g. "first build" or the changed inputs.
fn reason(run: &Run, options: &Options) -> String {
    if run.first {
        "first build".into()
    } else if run.resuming {
        "resuming".into()
    } else if run.changed.is_empty() {
        if options.force.iter().any(|f| f == run.step.name) { "forced".into() } else { "its code in build/src".into() }
    } else {
        let shown = run.changed.iter().take(6).cloned().collect::<Vec<_>>().join(" ");
        if run.changed.len() > 6 { shown + " …" } else { shown }
    }
}

/// Gather the build settings steps may depend on, passed to them as environment variables.
///
/// # Arguments
///
/// * `options` - what to build
/// * `commits` - the snapshot commits, which the image records
///
/// # Returns
///
/// Each setting's value, empty when off.
fn settings(options: &Options, commits: &[(String, String)]) -> BTreeMap<&'static str, String> {
    let keys: Vec<u8> = ["lastfm-api.json", "report-upload.json"].iter()
        .filter_map(|n| fs::read(Path::new("/keys").join(n)).ok()).flatten().collect();
    let stock: Vec<String> = files(Path::new("/stock")).iter()
        .filter_map(|p| fs::metadata(p).ok().map(|m| format!("{} {} {:?}", p.display(), m.len(), m.modified().ok())))
        .collect();
    let on = |flag: bool| if flag { "1".to_string() } else { String::new() };
    BTreeMap::from([
        ("COMMIT", commits.iter().map(|(_, c)| c.as_str()).collect::<Vec<_>>().join(" ")),
        ("PUBLIC", on(options.public)),
        ("FAST", on(options.fast)),
        ("KEYS", sha256(keys)),
        ("OWNER_KEY", if options.public { String::new() } else { fs::read_to_string("/owner.pub").unwrap_or_default().trim().to_string() }),
        ("STOCK", sha256(stock.join("\n"))),
        ("PUSH_KEY", fs::read_to_string(PUSH_KEY).unwrap_or_default()),
    ])
}

/// Order the wanted steps after every step they need.
///
/// # Arguments
///
/// * `targets` - steps, "all" (the package) or "boot" (the images); none means "all"
///
/// # Returns
///
/// The steps, needed ones first.
///
/// # Errors
///
/// Fails on an unknown step or steps that need each other.
fn plan(targets: &[String]) -> Result<Vec<&'static Step>> {
    fn visit(name: &str, chain: &mut Vec<String>, result: &mut Vec<&'static Step>) -> Result<()> {
        if chain.iter().any(|c| c == name) {
            bail!("the steps need each other: {} -> {name}", chain.join(" -> "));
        }
        if result.iter().any(|s| s.name == name) {
            return Ok(());
        }
        let step = ALL.iter().find(|s| s.name == name).with_context(|| {
            format!("no step {name:?}; steps: {}", ALL.iter().map(|s| s.name).collect::<Vec<_>>().join(", "))
        })?;
        chain.push(name.to_string());
        for need in step.needs {
            visit(need, chain, result)?;
        }
        chain.pop();
        result.push(step);
        Ok(())
    }
    let mut result = Vec::new();
    let targets: Vec<&str> = if targets.is_empty() { vec!["all"] } else { targets.iter().map(String::as_str).collect() };
    for target in targets {
        let name = match target { "all" => "package", "boot" => "artifacts", other => other };
        visit(name, &mut Vec::new(), &mut result)?;
    }
    Ok(result)
}

/// List what changed between two records of a step.
///
/// # Arguments
///
/// * `now`, `last` - the current record and the one of the last successful run
///
/// # Returns
///
/// Changed inputs by path (the repository for the whole of it), `dep:<step>` and `setting:<name>`.
fn changes(now: &Value, last: &Value) -> Vec<String> {
    let mut changed = BTreeSet::new();
    for (field, prefix) in [("inputs", ""), ("needs", "dep:"), ("settings", "setting:")] {
        let (a, b) = (now[field].as_object().cloned().unwrap_or_default(), last[field].as_object().cloned().unwrap_or_default());
        for name in a.keys().chain(b.keys()).filter(|n| a.get(*n) != b.get(*n)) {
            changed.insert(match name.split_once(':') {
                Some((repo, "")) if field == "inputs" => repo.to_string(),
                Some((_, path)) if field == "inputs" => path.to_string(),
                _ => format!("{prefix}{name}"),
            });
        }
    }
    changed.into_iter().collect()
}

/// Run the steps, each as soon as the steps it needs or runs after are done, showing their progress.
///
/// # Arguments
///
/// * `runs` - the steps to run
/// * `settings` - the build settings
///
/// # Errors
///
/// Fails if a step fails; the others that already started finish first.
fn execute(runs: Vec<Run>, settings: &BTreeMap<&str, String>) -> Result<()> {
    fs::create_dir_all(LOGS)?;
    let names: BTreeSet<&str> = runs.iter().map(|r| r.step.name).collect();
    let waits = |run: &Run| -> BTreeSet<&str> {
        run.step.needs.iter().chain(run.step.after).copied().filter(|n| names.contains(n)).collect()
    };
    let mut progress = Progress::new();
    let mut pending = runs;
    let mut running: Vec<(Run, Child, Instant)> = Vec::new();
    let (mut done, mut failed) = (BTreeSet::new(), Vec::new());
    while !pending.is_empty() || !running.is_empty() {
        while let Some(i) = pending.iter().position(|r| failed.is_empty() && waits(r).is_subset(&done)) {
            let run = pending.remove(i);
            let child = start(&run, settings)?;
            running.push((run, child, Instant::now()));
        }
        if running.is_empty() {
            break;
        }
        let Some(i) = running.iter_mut().position(|(_, child, _)| child.try_wait().is_ok_and(|s| s.is_some())) else {
            let logs: Vec<PathBuf> = running.iter().map(|(run, ..)| log(run.step.name)).collect();
            let rows: Vec<Row> = running.iter().zip(&logs).map(|((run, _, since), log)| Row {
                name: run.step.name, elapsed: since.elapsed(), expected: run.expected, log,
            }).collect();
            progress.draw(&rows);
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        let (run, mut child, since) = running.remove(i);
        let took = since.elapsed();
        let name = run.step.name;
        if child.wait()?.success() {
            finish(run, took)?;
            done.insert(name);
            progress.line(&format!("{name:14} {:.1} s", took.as_secs_f64()));
        } else {
            failed.push(name);
            let text = fs::read_to_string(log(name)).unwrap_or_default();
            let lines: Vec<&str> = text.lines().collect();
            progress.line(&format!("{name:14} failed after {:.1} s; the end of out/logs/{name}.log:\n{}",
                                   took.as_secs_f64(), lines[lines.len().saturating_sub(40)..].join("\n")));
        }
    }
    if !failed.is_empty() {
        bail!("{} failed", failed.join(", "));
    }
    Ok(())
}

/// Name a step's log.
///
/// # Arguments
///
/// * `name` - the step
///
/// # Returns
///
/// The log's path, out/logs/<step>.log.
fn log(name: &str) -> PathBuf {
    Path::new(LOGS).join(format!("{name}.log"))
}

/// Start a step in the background, its output going to out/logs/<step>.log.
///
/// # Arguments
///
/// * `run` - the step and its changes
/// * `settings` - the build settings
///
/// # Returns
///
/// The running step.
///
/// # Errors
///
/// Fails if the log or the attempt record cannot be written or the step cannot start.
fn start(run: &Run, settings: &BTreeMap<&str, String>) -> Result<Child> {
    fs::write(format!("{STATE}/attempt-{}", run.step.name), &run.key)?;
    let log = File::create(log(run.step.name))?;
    Ok(Command::new(std::env::current_exe()?)
        .args(["step", run.step.name])
        .envs(run.step.settings.iter().map(|s| (*s, settings[s].as_str())))
        .env("Y2B_CHANGED", run.changed.join(" "))
        .env("Y2B_FIRST", if run.first { "1" } else { "" })
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()?)
}

/// Record a step's successful run and how long it took, so the next build skips it while its key stays
/// the same and knows how long it takes.
///
/// # Arguments
///
/// * `run` - the step that succeeded
/// * `took` - how long it ran
///
/// # Errors
///
/// Fails if the record cannot be written.
fn finish(run: Run, took: Duration) -> Result<()> {
    let name = run.step.name;
    let mut record = run.record;
    record["key"] = json!(run.key);
    record["seconds"] = json!(took.as_secs_f64());
    fs::write(format!("{STATE}/step-{name}.json"), serde_json::to_string_pretty(&record)?)?;
    fs::remove_file(format!("{STATE}/attempt-{name}"))?;
    let _ = fs::remove_file(format!("{STATE}/unit-{name}.json"));
    Ok(())
}

/// Take the build lock, so only one build runs at a time.
///
/// # Returns
///
/// The lock file, held until dropped.
///
/// # Errors
///
/// Fails if another build holds the lock.
fn lock() -> Result<File> {
    let file = File::create(format!("{STATE}/lock"))?;
    if file.try_lock().is_err() {
        bail!("another build is running");
    }
    Ok(file)
}
