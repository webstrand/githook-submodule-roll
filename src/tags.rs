//! Finds the tag that a submodule rolls forward to from the commit that its parent pins.

use crate::replaces::Replaces;
use crate::repo::Repo;
use crate::runner::Error;
use crate::version::Version;

/// The tag that a pinned commit rolls forward to.
pub struct Target {
    /// The highest version tagged at or before the pin, which the target is compatible with.
    pub base: String,
    pub tag: String,
    pub commit: String,
    /// The version of the pin that a rewrite made which the tag holds, when it does not hold the
    /// pin.
    pub rewritten: Option<String>,
}

/// The tag that a submodule pinned at `pin` rolls forward to: of the tags that contain the pin,
/// or a version of it that the replaces notes say a rewrite made, the highest release that is
/// compatible with, and above, the base, the highest version tagged at or before the pin. A pin
/// with no tag before it, or that the repository lacks, takes its base from its versions instead.
/// A tag that holds no version of the pin, such as one on a branch that it is not on, is never
/// the target, so that the roll forward keeps the pin's changes. `None` when there is no base,
/// or no tag above it.
pub fn target(repo: &Repo, pin: &str, replaces: &Replaces) -> Result<Option<Target>, Error> {
    let has_pin = repo.commit(pin)?.is_some();
    let mut held = Vec::new();
    if has_pin {
        held.push(pin.to_owned());
    }
    for version in replaces.versions_of(pin) {
        if repo.commit(&version)?.is_some() {
            held.push(version);
        }
    }
    if held.is_empty() {
        return Ok(None);
    }
    let mut base = None;
    if has_pin {
        base = versions(repo, "--merged", &held[..1])?.into_iter().max();
    }
    if base.is_none() {
        base = versions(repo, "--merged", &held)?.into_iter().max();
    }
    let Some((base, base_tag)) = base else {
        return Ok(None);
    };
    let Some((_, tag)) = versions(repo, "--contains", &held)?
        .into_iter()
        .filter(|(version, _)| {
            !version.is_prerelease() && base.is_compatible(version) && *version > base
        })
        .max()
    else {
        return Ok(None);
    };
    let target = repo.stdout(["rev-parse", &format!("refs/tags/{tag}^{{commit}}")])?;
    let holds = |commit: &str| -> Result<bool, Error> {
        Ok(repo
            .stdout_if_succeeds(["merge-base", "--is-ancestor", commit, &target])?
            .is_some())
    };
    let mut rewritten = None;
    if !(has_pin && holds(pin)?) {
        for version in held.iter().filter(|version| *version != pin) {
            if holds(version)? {
                rewritten = Some(version.clone());
                break;
            }
        }
    }
    Ok(Some(Target {
        base: base_tag,
        tag,
        commit: target,
        rewritten,
    }))
}

/// The tags that name versions, of those that `git tag` lists with `filter` given for each of
/// `commits`, which it lists when they match any one.
fn versions(
    repo: &Repo,
    filter: &str,
    commits: &[String],
) -> Result<Vec<(Version, String)>, Error> {
    let mut args = vec!["tag", "--list"];
    for commit in commits {
        args.extend([filter, commit.as_str()]);
    }
    Ok(repo
        .stdout(args)?
        .lines()
        .filter_map(|tag| Version::parse(tag).map(|version| (version, tag.to_owned())))
        .collect())
}
