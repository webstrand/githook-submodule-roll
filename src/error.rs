/// A reason to stop, displayed after the name of the program.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Git(#[from] crate::runner::Error),
    #[error(
        "the post-rewrite hook of githook-replaces-index is not in the git config; without it, the rewrite of the pins is not recorded in the replaces notes, and the commits that held them cannot be found from their rewrites; install it, or run with --dry-run"
    )]
    NoReplacesIndex,
}
