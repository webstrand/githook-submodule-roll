//! Rewrites the history of branches of the parent so that each pin that a rewrite of its
//! submodule replaced, and that the submodule's remotes no longer hold, names the version of it
//! that they hold instead, as the replaces notes tell. The pins then no longer rely on the notes.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;

use crate::error;
use crate::replaces::Replaces;
use crate::repo::Repo;
use crate::runner::{self, Error};

const NAME: &str = "githook-submodule-roll";

pub struct Options {
    pub branches: Vec<String>,
    pub dry_run: bool,
}

/// The program that githook-replaces-index registers as a post-rewrite hook.
const REPLACES_INDEX_HOOK: &str = "githook-replaces-index-post-rewrite";

/// Rewrites `options.branches`, or the current branch when none is given. Refuses, unless
/// `options.dry_run`, when the hook of githook-replaces-index is not configured to record the
/// rewrite.
pub fn rewrite_pins(repo: &Repo, options: Options) -> Result<(), error::Error> {
    if !options.dry_run && !records_rewrites(repo)? {
        return Err(error::Error::NoReplacesIndex);
    }
    Ok(rewrite(repo, options)?)
}

/// Whether a post-rewrite hook in the git config runs the hook of githook-replaces-index.
fn records_rewrites(repo: &Repo) -> Result<bool, Error> {
    let Some(names) = repo.stdout_if_succeeds(["hook", "list", "post-rewrite"])? else {
        return Ok(false);
    };
    for name in names.lines() {
        let commands =
            repo.stdout_if_succeeds(["config", "--get-all", &format!("hook.{name}.command")])?;
        if commands.is_some_and(|commands| commands.contains(REPLACES_INDEX_HOOK)) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn rewrite(repo: &Repo, options: Options) -> Result<(), Error> {
    let refs = if options.branches.is_empty() {
        vec![repo.stdout(["symbolic-ref", "HEAD"])?]
    } else {
        options
            .branches
            .iter()
            .map(|branch| repo.stdout(["rev-parse", "--symbolic-full-name", branch]))
            .collect::<Result<_, _>>()?
    };
    let tips: Vec<String> = refs
        .iter()
        .map(|name| repo.stdout(["rev-parse", "--verify", &format!("{name}^{{commit}}")]))
        .collect::<Result<_, _>>()?;

    let mut pins = Pins::new(repo);
    let mut rewritten: HashMap<String, String> = HashMap::new();
    let mut order = Vec::new();
    let mut args = vec!["rev-list", "--reverse", "--topo-order", "--parents"];
    args.extend(tips.iter().map(String::as_str));
    for line in repo.stdout(args)?.lines() {
        let mut ids = line.split(' ');
        let Some(commit) = ids.next() else {
            continue;
        };
        let parents: Vec<&str> = ids.collect();
        let mut changes = Vec::new();
        for (path, pin) in gitlinks(repo, commit)? {
            if let Some(new) = pins.replacement(&path, &pin)? {
                changes.push((path, new));
            }
        }
        let new_parents: Vec<&str> = parents
            .iter()
            .map(|parent| rewritten.get(*parent).map_or(*parent, String::as_str))
            .collect();
        if changes.is_empty() && new_parents == parents {
            continue;
        }
        let new = if options.dry_run {
            format!("rewritten-{commit}")
        } else {
            write_commit(repo, commit, &changes, &new_parents)?
        };
        rewritten.insert(commit.to_owned(), new);
        order.push(commit.to_owned());
    }

    for (path, pin, new) in &pins.reported {
        eprintln!("{NAME}: {path}: {pin} -> {new}");
    }
    if order.is_empty() {
        eprintln!("{NAME}: no pin to rewrite");
        return Ok(());
    }
    if options.dry_run {
        eprintln!("{NAME}: would rewrite {} commits", order.len());
        return Ok(());
    }

    let head = repo.stdout_if_succeeds(["symbolic-ref", "--quiet", "HEAD"])?;
    for (name, tip) in refs.iter().zip(&tips) {
        let Some(new) = rewritten.get(tip) else {
            continue;
        };
        repo.stdout([
            "update-ref",
            "-m",
            "githook-submodule-roll rewrite-pins",
            name,
            new,
            tip,
        ])?;
        eprintln!("{NAME}: rewrote {name}");
        if head.as_deref() == Some(name.as_str()) {
            repo.stdout(["update-ref", "ORIG_HEAD", tip])?;
            update_index(repo, &pins)?;
        }
    }
    warn_published(repo, &order)?;
    run_post_rewrite(repo, &order, &rewritten)
}

/// The gitlinks of `commit`'s tree, as their paths and the commits they pin.
fn gitlinks(repo: &Repo, commit: &str) -> Result<Vec<(String, String)>, Error> {
    let tree = repo.stdout(["ls-tree", "-r", "-z", commit])?;
    Ok(tree
        .split('\0')
        .filter_map(|entry| {
            let (meta, path) = entry.split_once('\t')?;
            let mut fields = meta.split(' ');
            let (mode, id) = (fields.next()?, fields.nth(1)?);
            (mode == "160000").then(|| (path.to_owned(), id.to_owned()))
        })
        .collect())
}

/// The commit made from `commit` with the gitlinks of `changes` and the parents `parents`. Its
/// other headers and message are copied as they are, except a signature, which would no longer
/// match.
fn write_commit(
    repo: &Repo,
    commit: &str,
    changes: &[(String, String)],
    parents: &[&str],
) -> Result<String, Error> {
    let raw = runner::bytes(&mut repo.git(["cat-file", "commit", commit]))?;
    let split = raw
        .windows(2)
        .position(|w| w == b"\n\n")
        .map_or(raw.len(), |i| i + 1);
    let (headers, message) = raw.split_at(split);
    let headers = String::from_utf8_lossy(headers);

    let mut tree = None;
    let mut rest = String::new();
    let mut in_signature = false;
    for line in headers.lines() {
        if let Some(continuation) = line.strip_prefix(' ') {
            if !in_signature {
                rest.push(' ');
                rest.push_str(continuation);
                rest.push('\n');
            }
            continue;
        }
        in_signature = false;
        match line.split_once(' ') {
            Some(("tree", id)) => tree = Some(id.to_owned()),
            Some(("parent", _)) => {}
            Some(("gpgsig" | "gpgsig-sha256", _)) => in_signature = true,
            _ => {
                rest.push_str(line);
                rest.push('\n');
            }
        }
    }
    let tree = tree.unwrap_or_default();
    let tree = if changes.is_empty() {
        tree
    } else {
        write_tree(repo, &tree, changes)?
    };

    let mut object = format!("tree {tree}\n").into_bytes();
    for parent in parents {
        object.extend(format!("parent {parent}\n").into_bytes());
    }
    object.extend(rest.into_bytes());
    object.extend(message);
    let id = runner::bytes_with_input(
        &mut repo.git(["hash-object", "-t", "commit", "-w", "--stdin"]),
        &object,
    )?;
    Ok(String::from_utf8_lossy(&id).trim().to_owned())
}

/// The tree made from `tree` with the gitlinks of `changes`, through a temporary index.
fn write_tree(repo: &Repo, tree: &str, changes: &[(String, String)]) -> Result<String, Error> {
    let tmp = tempfile::NamedTempFile::new().map_err(|source| Error::Spawn {
        command: "a temporary index".to_owned(),
        source,
    })?;
    let index = tmp.path().to_owned();
    let with_index = |args: &[&str]| {
        let mut command = repo.git(args);
        command.env("GIT_INDEX_FILE", &index);
        command
    };
    runner::bytes(&mut with_index(&["read-tree", tree]))?;
    let input: String = changes
        .iter()
        .map(|(path, pin)| format!("160000 {pin}\t{path}\n"))
        .collect();
    runner::bytes_with_input(
        &mut with_index(&["update-index", "--index-info"]),
        input.as_bytes(),
    )?;
    let id = runner::bytes(&mut with_index(&["write-tree"]))?;
    Ok(String::from_utf8_lossy(&id).trim().to_owned())
}

/// Gives each gitlink of the index that holds a rewritten pin its replacement, so that the
/// index does not undo the rewrite of the current branch.
fn update_index(repo: &Repo, pins: &Pins) -> Result<(), Error> {
    let index = repo.stdout(["ls-files", "--stage", "-z"])?;
    let mut input = String::new();
    for entry in index.split('\0') {
        let Some((meta, path)) = entry.split_once('\t') else {
            continue;
        };
        if let ["160000", pin, "0"] = meta.split(' ').collect::<Vec<_>>()[..]
            && let Some(Some(new)) = pins.decided.get(&(path.to_owned(), pin.to_owned()))
        {
            input.push_str(&format!("160000 {new}\t{path}\n"));
        }
    }
    if !input.is_empty() {
        runner::bytes_with_input(
            &mut repo.git(["update-index", "--index-info"]),
            input.as_bytes(),
        )?;
    }
    Ok(())
}

/// Warns when commits that were rewritten are on a remote-tracking branch, which a push must then
/// force.
fn warn_published(repo: &Repo, rewritten: &[String]) -> Result<(), Error> {
    let mut args = vec!["for-each-ref", "--format=%(refname:short)"];
    let contains: Vec<String> = rewritten
        .iter()
        .map(|commit| format!("--contains={commit}"))
        .collect();
    args.extend(contains.iter().map(String::as_str));
    args.push("refs/remotes");
    let remotes = repo.stdout(args)?;
    for remote in remotes.lines() {
        eprintln!(
            "{NAME}: warning: {remote} holds commits that were rewritten; a push of them must force, as with --force-with-lease"
        );
    }
    Ok(())
}

/// Runs the post-rewrite hooks of the parent as after a rebase, so that a hook such as that of
/// githook-replaces-index records the rewrite.
fn run_post_rewrite(
    repo: &Repo,
    order: &[String],
    rewritten: &HashMap<String, String>,
) -> Result<(), Error> {
    let tmp = tempfile::NamedTempFile::new().map_err(|source| Error::Spawn {
        command: "a temporary file".to_owned(),
        source,
    })?;
    let lines: String = order
        .iter()
        .map(|old| format!("{old} {}\n", rewritten[old]))
        .collect();
    fs::write(tmp.path(), lines).map_err(|source| Error::Spawn {
        command: "a temporary file".to_owned(),
        source,
    })?;
    let to_stdin = format!("--to-stdin={}", tmp.path().display());
    let output = runner::run(&mut repo.git([
        "hook",
        "run",
        "--ignore-missing",
        &to_stdin,
        "post-rewrite",
        "--",
        "rebase",
    ]))?;
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        eprintln!(
            "{NAME}: warning: a post-rewrite hook failed ({})",
            output.status
        );
    }
    Ok(())
}

/// The replacements of the pins of each submodule path, decided once each.
struct Pins<'a> {
    repo: &'a Repo,
    subs: BTreeMap<String, Option<(Repo, Replaces)>>,
    decided: BTreeMap<(String, String), Option<String>>,
    reported: Vec<(String, String, String)>,
}

impl<'a> Pins<'a> {
    fn new(repo: &'a Repo) -> Pins<'a> {
        Pins {
            repo,
            subs: BTreeMap::new(),
            decided: BTreeMap::new(),
            reported: Vec::new(),
        }
    }

    /// The commit that replaces `pin` at `path`, when the submodule's remotes no longer hold the
    /// pin and the notes lead to a single newest version of it that they hold.
    fn replacement(&mut self, path: &str, pin: &str) -> Result<Option<String>, Error> {
        let key = (path.to_owned(), pin.to_owned());
        if let Some(decided) = self.decided.get(&key) {
            return Ok(decided.clone());
        }
        let decided = self.decide(path, pin)?;
        if let Some(new) = &decided {
            self.reported
                .push((path.to_owned(), pin.to_owned(), new.clone()));
        }
        self.decided.insert(key, decided.clone());
        Ok(decided)
    }

    fn decide(&mut self, path: &str, pin: &str) -> Result<Option<String>, Error> {
        if !self.subs.contains_key(path) {
            let sub = self.repo.submodule(path);
            let entry = if sub.is_checked_out() {
                let replaces = Replaces::read(&sub)?;
                Some((sub, replaces))
            } else {
                eprintln!("{NAME}: {path}: not checked out; its pins are left as they are");
                None
            };
            self.subs.insert(path.to_owned(), entry);
        }
        let Some((sub, replaces)) = &self.subs[path] else {
            return Ok(None);
        };
        if sub.is_published(pin)? {
            return Ok(None);
        }
        let mut versions = BTreeSet::new();
        for version in replaces.versions_of(pin) {
            if sub.is_published(&version)? {
                versions.insert(version);
            }
        }
        // The newest are those that no other published version replaces.
        let newest: Vec<String> = versions
            .iter()
            .filter(|version| {
                !replaces
                    .versions_of(version)
                    .iter()
                    .any(|newer| versions.contains(newer))
            })
            .cloned()
            .collect();
        match &newest[..] {
            [] => Ok(None),
            [new] => Ok(Some(new.clone())),
            _ => {
                eprintln!(
                    "{NAME}: {path}: warning: {pin} was rewritten as several commits that are published, {}; it is left as it is",
                    newest.join(", ")
                );
                Ok(None)
            }
        }
    }
}
