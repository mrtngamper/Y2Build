//! Copy the working trees of Y2Linux and Y2Reborn, uncommitted changes included, into /y2/<repo> as one
//! git commit each. Only the files that changed are rewritten, so make sees only those.

use anyhow::Result;
use std::path::Path;
use xshell::{Shell, cmd};

/// The repositories the build snapshots.
pub const REPOS: [&str; 2] = ["Y2Linux", "Y2Reborn"];

/// Take the snapshot of one repository.
///
/// # Arguments
///
/// * `sh` - the shell to run git in
/// * `name` - the repository, one of [`REPOS`]
///
/// # Returns
///
/// The snapshot's commit and whether it holds uncommitted changes.
///
/// # Errors
///
/// Fails if a git command fails.
pub fn take(sh: &Shell, name: &str) -> Result<(String, bool)> {
    let source = format!("/y2/Y2Build/{name}");
    let target = format!("/y2/{name}");
    let (tree, head) = stage_working_tree(sh, &source, name)?;
    let head_tree = "HEAD^{tree}";
    let dirty = tree != cmd!(sh, "git -C {source} rev-parse {head_tree}").quiet().read()?;
    let commit = if dirty { commit_tree(sh, &source, &tree, &head)? } else { head };
    if !Path::new(&target).join(".git").is_dir() {
        cmd!(sh, "git -C {target} init --quiet").quiet().run()?;
    }
    let current = cmd!(sh, "git -C {target} rev-parse --verify --quiet HEAD").quiet().ignore_status().read()?;
    if current != commit {
        cmd!(sh, "git -C {source} push --quiet --force {target} {commit}:refs/y2b/build").quiet().run()?;
        cmd!(sh, "git -C {target} checkout --quiet --force --detach {commit}").quiet().run()?;
    }
    Ok((commit, dirty))
}

/// Stage the working tree, untracked files included, in a private index that starts from HEAD and
/// keeps the file stats of the last run, so the user's index stays untouched and unchanged files are
/// not read again.
///
/// # Arguments
///
/// * `sh` - the shell to run git in
/// * `source` - the repository
/// * `name` - its name, which names the private index
///
/// # Returns
///
/// The working tree's git tree and HEAD.
///
/// # Errors
///
/// Fails if a git command fails.
fn stage_working_tree(sh: &Shell, source: &str, name: &str) -> Result<(String, String)> {
    let index = format!("/y2/state/index-{name}");
    let reset = Path::new(&index).exists().then_some("--reset");
    let _index = sh.push_env("GIT_INDEX_FILE", &index);
    cmd!(sh, "git -C {source} read-tree {reset...} HEAD").quiet().run()?;
    cmd!(sh, "git -C {source} add --all").quiet().run()?;
    let tree = cmd!(sh, "git -C {source} write-tree").quiet().read()?;
    let head = cmd!(sh, "git -C {source} rev-parse HEAD").quiet().read()?;
    Ok((tree, head))
}

/// Commit a tree on HEAD with fixed author and date, so the same tree on the same HEAD always gives
/// the same commit.
///
/// # Arguments
///
/// * `sh` - the shell to run git in
/// * `source` - the repository
/// * `tree` - the tree to commit
/// * `head` - the parent commit
///
/// # Returns
///
/// The commit.
///
/// # Errors
///
/// Fails if a git command fails.
fn commit_tree(sh: &Shell, source: &str, tree: &str, head: &str) -> Result<String> {
    let date = cmd!(sh, "git -C {source} log -1 --format=%cI HEAD").quiet().read()?;
    let _who = [
        ("GIT_AUTHOR_NAME", "y2b"), ("GIT_AUTHOR_EMAIL", "y2b@localhost"), ("GIT_AUTHOR_DATE", date.as_str()),
        ("GIT_COMMITTER_NAME", "y2b"), ("GIT_COMMITTER_EMAIL", "y2b@localhost"), ("GIT_COMMITTER_DATE", date.as_str()),
    ].map(|(name, value)| sh.push_env(name, value));
    Ok(cmd!(sh, "git -C {source} commit-tree {tree} -p {head} -m 'y2b: uncommitted changes'").quiet().read()?)
}

/// Look up the git object of each input in the snapshots.
///
/// # Arguments
///
/// * `sh` - the shell to run git in
/// * `inputs` - `<repo>:<path>` entries; a path ending in `/*` stands for each child of the folder
/// * `commits` - the snapshot commit of each repository
///
/// # Returns
///
/// Each input with its object id, "missing" when absent.
///
/// # Errors
///
/// Fails if a git command fails.
pub fn object_ids(sh: &Shell, inputs: &[&str], commits: &[(String, String)]) -> Result<Vec<(String, String)>> {
    let mut ids = Vec::new();
    for input in inputs {
        let (repo, path) = input.split_once(':').expect("inputs are <repo>:<path>");
        let commit = &commits.iter().find(|(name, _)| name == repo).expect("a snapshot repository").1;
        let target = format!("/y2/{repo}");
        if let Some(folder) = path.strip_suffix("/*") {
            let listing = cmd!(sh, "git -C {target} ls-tree {commit} {folder}/").quiet().read()?;
            for line in listing.lines() {
                let (meta, child) = line.split_once('\t').expect("ls-tree output");
                ids.push((format!("{repo}:{child}"), meta.split_whitespace().nth(2).unwrap_or_default().to_string()));
            }
        } else {
            let spec = if path.is_empty() { format!("{commit}^{{tree}}") } else { format!("{commit}:{path}") };
            let id = cmd!(sh, "git -C {target} rev-parse --verify --quiet {spec}").quiet().ignore_status().read()?;
            ids.push((input.to_string(), if id.is_empty() { "missing".into() } else { id }));
        }
    }
    Ok(ids)
}
