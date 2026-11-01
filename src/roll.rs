//! Rolls each submodule forward from the commit that its parent pins to the tag that `tags`
//! finds, and the submodules within it, in turn.

use crate::replaces::Replaces;
use crate::repo::Repo;
use crate::runner::{self, Error};
use crate::tags;

const NAME: &str = "githook-submodule-roll";

/// Rolls forward each checked-out submodule that the index of `repo` pins. `prev` are the commits
/// that `repo` was at before, whose pins a submodule may still follow.
pub fn roll(repo: &Repo, prev: &[String]) -> Result<(), Error> {
    let index = repo.stdout(["ls-files", "--stage", "-z"])?;
    for entry in index.split('\0').filter(|entry| !entry.is_empty()) {
        let Some((meta, path)) = entry.split_once('\t') else {
            continue;
        };
        // A gitlink is merged by stage 0 alone, and a conflict leaves the submodule as it is.
        if let ["160000", pin, "0"] = meta.split(' ').collect::<Vec<_>>()[..] {
            submodule(repo, path, pin, prev)?;
        }
    }
    Ok(())
}

fn submodule(parent: &Repo, path: &str, pin: &str, prev: &[String]) -> Result<(), Error> {
    let sub = parent.submodule(path);
    if !sub.is_checked_out() {
        return Ok(());
    }
    let head = sub.stdout(["rev-parse", "HEAD"])?;
    let short = |commit: &str| commit[..commit.len().min(12)].to_owned();
    // A submodule on a branch is the operator's to move.
    let on_branch = sub
        .stdout_if_succeeds(["symbolic-ref", "--quiet", "HEAD"])?
        .is_some();
    if on_branch {
        return roll(&sub, std::slice::from_ref(&head));
    }
    let replaces = Replaces::read(&sub)?;
    let target = tags::target(&sub, pin, &replaces)?;
    // A pin that a rewrite replaced may be gone from the remote, and is rolled forward from the
    // versions of it that the notes name.
    if target.is_none() && sub.commit(pin)?.is_none() {
        eprintln!(
            "{NAME}: {}: the pinned commit {} is not in the submodule; git submodule update fetches it",
            sub.path(),
            short(pin)
        );
        return Ok(());
    }
    // A pin that no remote branch or tag holds is one that was rewritten or never pushed, and a
    // clone that fetches the remote cannot check it out.
    if target
        .as_ref()
        .is_none_or(|target| target.rewritten.is_none())
        && sub.commit(pin)?.is_some()
        && sub.has_remote_branches()?
        && !sub.is_published(pin)?
    {
        eprintln!(
            "{NAME}: {}: warning: the pinned commit {} is on no branch or tag of the submodule's remotes; it was rewritten or never pushed, and other clones cannot check it out",
            sub.path(),
            short(pin)
        );
    }
    let wanted = target.as_ref().map_or(pin, |target| target.commit.as_str());
    if head != wanted {
        if !follows(parent, &sub, &replaces, path, pin, prev, &head)? {
            return roll(&sub, std::slice::from_ref(&head));
        }
        let changes = sub.stdout([
            "status",
            "--porcelain",
            "--untracked-files=no",
            "--ignore-submodules=all",
        ])?;
        if !changes.is_empty() {
            eprintln!(
                "{NAME}: {}: has uncommitted changes; left at {}",
                sub.path(),
                short(&head)
            );
            return roll(&sub, std::slice::from_ref(&head));
        }
        // The submodules within it are not moved with it: they are rolled forward below, and
        // left as they are when the operator moved them, as a recursive checkout would not.
        // Git refuses to overwrite untracked files, and the submodule is then left as it is.
        let checkout = runner::run(&mut sub.git([
            "-c",
            "submodule.recurse=false",
            "checkout",
            "--quiet",
            "--detach",
            wanted,
        ]))?;
        if !checkout.status.success() {
            let label = target
                .as_ref()
                .map_or_else(|| short(pin), |target| target.tag.clone());
            eprintln!(
                "{NAME}: {}: could not check out {label}; left at {}:\n{}",
                sub.path(),
                short(&head),
                String::from_utf8_lossy(&checkout.stderr).trim_end()
            );
            return roll(&sub, std::slice::from_ref(&head));
        }
        match &target {
            Some(target) => {
                eprintln!(
                    "{NAME}: {}: rolled forward from {}, after {}, to {}{}",
                    sub.path(),
                    short(pin),
                    target.base,
                    target.tag,
                    if target.rewritten.is_some() {
                        ", which holds a rewrite of it"
                    } else {
                        ""
                    }
                );
                if let Some(version) = &target.rewritten {
                    eprintln!(
                        "{NAME}: {}: warning: only the replaces notes tie the pin {} to {}, which stands in its place; the notes may be lost, and the parent should pin {} instead, or rewrite its history with githook-submodule-roll rewrite-pins",
                        sub.path(),
                        short(pin),
                        short(version),
                        short(version)
                    );
                }
            }
            None => eprintln!(
                "{NAME}: {}: checked out the pinned {}",
                sub.path(),
                short(pin)
            ),
        }
    }
    roll(&sub, std::slice::from_ref(&head))
}

/// Whether the submodule, at `head`, follows its pin rather than a commit that the operator
/// checked out: whether it is at the pin, or at the pin of one of `prev` or the tag that it rolled
/// forward to. A checkout that does not recurse into submodules leaves them at the pins of `prev`.
fn follows(
    parent: &Repo,
    sub: &Repo,
    replaces: &Replaces,
    path: &str,
    pin: &str,
    prev: &[String],
    head: &str,
) -> Result<bool, Error> {
    if head == pin {
        return Ok(true);
    }
    for prev in prev {
        let Some(prev_pin) = parent.stdout_if_succeeds([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{prev}:{path}"),
        ])?
        else {
            continue;
        };
        if head == prev_pin
            || tags::target(sub, &prev_pin, replaces)?.is_some_and(|target| target.commit == head)
        {
            return Ok(true);
        }
    }
    Ok(false)
}
