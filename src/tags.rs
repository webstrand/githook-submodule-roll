//! Finds the tag that a submodule rolls forward to from the commit that its parent pins.

use crate::repo::Repo;
use crate::runner::Error;
use crate::version::Version;

/// The tag that a pinned commit rolls forward to.
pub struct Target {
    /// The highest version tagged at or before the pin, which the target is compatible with.
    pub base: String,
    pub tag: String,
    pub commit: String,
}

/// The tag that a submodule pinned at `pin` rolls forward to: of the tags that contain the pin,
/// the highest release that is compatible with, and above, the base, the highest version tagged
/// at or before the pin. A tag that does not contain the pin, such as one on a branch that it is
/// not on, is never the target, so that the roll forward keeps every commit of the pin. `None`
/// when there is no base, or no tag above it. The repository must have the pin.
pub fn target(repo: &Repo, pin: &str) -> Result<Option<Target>, Error> {
    let held = [pin.to_owned()];
    let Some((base, base_tag)) = versions(repo, "--merged", &held)?.into_iter().max() else {
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
    let commit = repo.stdout(["rev-parse", &format!("refs/tags/{tag}^{{commit}}")])?;
    Ok(Some(Target {
        base: base_tag,
        tag,
        commit,
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
