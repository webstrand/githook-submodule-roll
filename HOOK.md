# submodule roll

The submodule roll moves each submodule, after a checkout or a merge, from the commit that its parent pins to the highest release tag that is compatible with the pin. A pin thus names the oldest release that the parent works with, and the submodule runs the newest release that semver promises to be compatible with it.

The target of a pin is found from the submodule's tags:

- A tag names a version when it reads `v1.2.3`, `1.2.3`, `v1.9` or `v2`, with an optional pre-release label after `-` and build metadata after `+`. A missing minor or patch number is 0. Other tags are ignored.
- The base is the highest version tagged at or before the pin, which `git tag --merged` lists. A pin four commits after `v1.2.3` has the base `v1.2.3`, as has the commit `v1.2.3` itself.
- The target is the highest version above the base that is tagged at or after the pin, which `git tag --contains` lists, that is not a pre-release and is compatible with the base as cargo reads semver: a version from 1.0.0 on is compatible with those of the same major number, and a version before 1.0.0 with those of the same minor number. A tag on a branch that does not contain the pin is never the target, so that the roll forward keeps every commit of the pin.
- A pin without a base, or without a target, is checked out as it is.

Only the tags that the submodule's repository holds are read; the hook does not fetch. `git -C <submodule> fetch --tags` brings in the tags of later releases.

A submodule is rolled forward only while it follows its pin: when its `HEAD` is detached at the pin, or at the pin of the commit that the parent was at before or the target of that pin, as a checkout that does not recurse into submodules leaves it. A submodule on a branch, or detached at another commit, which the operator has checked out, is left as it is, as is a submodule with uncommitted changes to tracked files, or with untracked files that the target would overwrite, of each of which the hook prints a warning; untracked files that the target does not hold stay as they are. A submodule that is not checked out, or whose pinned commit its repository does not have, is left as it is too; `git submodule update` checks it out or fetches the commit. A submodule that is merged with a conflict is left as it is. A pin that no remote-tracking branch or tag of the submodule holds, in a submodule that has fetched a remote, draws a warning at each checkout: it was rewritten or never pushed, and a clone that fetches the remote cannot check it out.

The submodules within a submodule are rolled forward in turn, from the pins of the commit that the submodule is left at. The checkout that rolls a submodule forward does not recurse into them, whatever `submodule.recurse` says, so that each is moved only when it follows its pin, by the rules above.

The hook moves only the submodule's `HEAD`; the parent's index keeps the pin. `git status` in the parent thus shows each rolled submodule as modified, and `git add` of the submodule, or `git commit -a`, records the target as its pin. `git submodule update` checks out the pin again, and the next checkout or merge rolls it forward.

## Events

- `post-checkout`: after a checkout of a branch or a commit, including `git switch`, `git checkout --recurse-submodules` and `git worktree add`. A checkout of files does not roll submodules forward.
- `post-merge`: after `git merge` and `git pull` that merges, from the pins of `ORIG_HEAD`.
- `post-rewrite`: after a rebase, including `git pull --rebase`, from the pins of `ORIG_HEAD` and of the commit that the rebase started from, which git checked out, and `post-checkout` rolled forward from, before it replayed the commits. A rebase that only fast-forwards runs `post-checkout` alone. An amend rolls nothing forward.

Git runs none of these after `git reset`, `git cherry-pick`, `git revert`, `git stash`, `git bisect`, `git submodule update` or a `git rebase --abort`, which may change the pins or move the submodules as well; nor after `git clone`, before the installer has run. `githook-submodule-roll roll`, run by hand in the parent, rolls forward the submodules that are at the pins of the index after such a command. A rebase that is aborted after it stopped leaves each submodule at the target of the commit it started from, which no longer follows a pin of the parent; `git submodule update` and `githook-submodule-roll roll` then roll it forward from the pin.

The hook cannot undo the command that ran it, which has finished. A failure of the hook, such as a git command that it could not run, is printed; after a checkout, it also makes `git checkout` or `git switch` exit with status 1, whereas a merge or a rebase exits as it would have.

## Installation

The hook is written in Rust, a crate that is a cargo workspace of its own, so that a repository may keep it as a submodule apart from its own build. `install.sh`, run from inside a checkout, installs it with `cargo install`, which needs a Rust toolchain, into `bin/githook-submodule-roll` in the repository's common git directory, and registers it in the repository's config as `hook.submodule-roll-post-checkout`, `hook.submodule-roll-post-merge` and `hook.submodule-roll-post-rewrite`, which run beside any other hooks of the events. Configured hooks need a git that supports `hook.<name>.event`; the hook is tested with git 2.56. The commands name the binary through `$(git rev-parse --git-common-dir)`, which the shell resolves each time git runs the hook, so the repository may move. It may be run again to update the hook. Run from inside a submodule, it refuses, since it would install into the submodule's repository rather than the one that mounts it; `INSTALL_SUBMODULE=1` installs into the submodule nonetheless. It then runs `githook-submodule-roll roll`, so that the submodules that are at their pins are rolled forward without waiting for a checkout. Each clone must run it once; worktrees share the hook and the config of their repository.
