# submodule roll

The submodule roll moves each submodule, after a checkout or a merge, from the commit that its parent pins to the highest release tag that is compatible with the pin. A pin thus names the oldest release that the parent works with, and the submodule runs the newest release that semver promises to be compatible with it.

The target of a pin is found from the submodule's tags:

- A tag names a version when it reads `v1.2.3`, `1.2.3`, `v1.9` or `v2`, with an optional pre-release label after `-` and build metadata after `+`. A missing minor or patch number is 0. Other tags are ignored.
- The base is the highest version tagged at or before the pin, which `git tag --merged` lists. A pin four commits after `v1.2.3` has the base `v1.2.3`, as has the commit `v1.2.3` itself.
- The target is the highest version above the base that is tagged at or after the pin, or at or after a version of the pin that a rewrite made, which `git tag --contains` lists, that is not a pre-release and is compatible with the base as cargo reads semver: a version from 1.0.0 on is compatible with those of the same major number, and a version before 1.0.0 with those of the same minor number. A tag on a branch that holds no version of the pin is never the target, so that the roll forward keeps the changes of the pin.
- A pin without a base, or without a target, is checked out as it is.

Only the tags that the submodule's repository holds are read; the hook does not fetch. `git -C <submodule> fetch --tags` brings in the tags of later releases.

A submodule is rolled forward only while it follows its pin: when its `HEAD` is detached at the pin, or at the pin of the commit that the parent was at before or the target of that pin, as a checkout that does not recurse into submodules leaves it. A submodule on a branch, or detached at another commit, which the operator has checked out, is left as it is, as is a submodule with uncommitted changes to tracked files, or with untracked files that the target would overwrite, of each of which the hook prints a warning; untracked files that the target does not hold stay as they are. A submodule that is not checked out, or whose pinned commit its repository does not have and whose replaces notes lead to no target, is left as it is too; `git submodule update` checks it out or fetches the commit. A submodule that is merged with a conflict is left as it is. A pin that no remote-tracking branch or tag of the submodule holds, in a submodule that has fetched a remote, draws a warning at each checkout: it was rewritten or never pushed, and a clone that fetches the remote cannot check it out.

The submodules within a submodule are rolled forward in turn, from the pins of the commit that the submodule is left at. The checkout that rolls a submodule forward does not recurse into them, whatever `submodule.recurse` says, so that each is moved only when it follows its pin, by the rules above.

The hook moves only the submodule's `HEAD`; the parent's index keeps the pin. `git status` in the parent thus shows each rolled submodule as modified, and `git add` of the submodule, or `git commit -a`, records the target as its pin. `git submodule update` checks out the pin again, and the next checkout or merge rolls it forward.

## Rewritten pins

A remote whose history is rewritten, by an amend, a rebase or `git filter-repo`, holds commits that stand in place of the ones that parents pin; no later tag contains the old commits, and the remote may no longer have them. Where the remote keeps the replaces notes of [`githook-replaces-index`](https://github.com/webstrand/githook-replaces-index), the hook follows them from the pin to each commit that a rewrite made from it, and from that commit to each one made from it in turn, as the replaces index's own queries do. A tag that contains any of them that the submodule's repository has may be the target, and the message that reports the roll forward says that the tag holds a rewrite of the pin. A pin that the repository lacks takes its base from its versions; one that it has takes its base from itself, so that a rewrite that moved the pin's changes past a major version does not carry the submodule with it, unless no tag is at or before the pin.

The notes are read from `refs/notes/replaces` and from `refs/notes/remotes/<remote>/replaces` in the submodule's repository, through `git notes list` and `git cat-file`; the programs of the replaces index need not be installed. A submodule's remote holds its notes in `refs/notes/replaces`, and `git fetch` brings them into `refs/notes/remotes/<remote>/replaces` when the remote's configuration holds the refspec that the replaces index names:

```
git -C <submodule> config --add remote.<remote>.fetch '+refs/notes/replaces*:refs/notes/remotes/<remote>/replaces*'
```

The refspec is a glob, so that a remote without replaces notes matches nothing rather than failing the fetch. `git submodule update`, which fetches with the remote's configuration, then brings in the notes with the commits.

The notes are a weak link: they are a ref beside the history, which a remote may not keep, a clone may not fetch, and a `git notes` command may overwrite. A roll forward that relies on them therefore prints a warning, which names the commit that stands in place of the pin and says that the parent should pin it instead, or rewrite its history with `rewrite-pins`.

## Rewriting the pins in history

`githook-submodule-roll rewrite-pins [--dry-run] [<branch>...]`, run in the parent, rewrites the history of the branches it names, or of the current branch, so that each pin that the notes say a rewrite replaced names the commit that stands in its place, and no longer relies on the notes:

- A pin is rewritten only when no remote-tracking branch or tag of the submodule holds it, and the notes lead from it to a single newest version that one does hold: a version that the notes say no other published version replaces. A pin that the remotes still hold is left as it is, since the notes record which commit was made from which, not that the old one was withdrawn; a pin with several newest versions is left as it is, with a warning. The submodule must be checked out, so that its notes and refs can be read; the pins of a submodule that is not checked out are left as they are, with a warning.
- Each commit that holds such a pin, or whose parents were rewritten, is written anew with the new gitlinks and parents. Its author, committer, dates and message are kept as they are; a signature is dropped, since it no longer matches. Each other commit is kept as it is.
- Each branch is moved to its rewrite only from the commit that it was read at. When the current branch is rewritten, `ORIG_HEAD` is set to the commit it was at, and each gitlink of the index that holds a rewritten pin is given the new one, so that the index does not undo the rewrite; the submodules' checkouts are not moved.
- The `post-rewrite` hooks of the parent are run as after a rebase, with a line `<old> <new>` for each rewritten commit, through `git hook run`, so that a hook such as that of the replaces index records the rewrite. A failing hook draws a warning.
- A remote-tracking branch that holds rewritten commits draws a warning: a push of the rewrite must force, as `git push --force-with-lease` does.

`rewrite-pins` refuses to rewrite unless a `post-rewrite` hook of the git config, as `git hook list post-rewrite` lists them, runs `githook-replaces-index-post-rewrite`: without it, nothing records which commit each rewritten one was made from, and the commits that held the old pins, such as those that other clones and branches still hold, can no longer be found from their rewrites. `--dry-run` prints the pins that would be rewritten, and the number of commits, and writes nothing; it does not need the hook. The pins of the submodules within a submodule are not rewritten, since they are part of the submodule's history.

## Events

- `post-checkout`: after a checkout of a branch or a commit, including `git switch`, `git checkout --recurse-submodules` and `git worktree add`. A checkout of files does not roll submodules forward.
- `post-merge`: after `git merge` and `git pull` that merges, from the pins of `ORIG_HEAD`.
- `post-rewrite`: after a rebase, including `git pull --rebase`, from the pins of `ORIG_HEAD` and of the commit that the rebase started from, which git checked out, and `post-checkout` rolled forward from, before it replayed the commits. A rebase that only fast-forwards runs `post-checkout` alone. An amend rolls nothing forward.

Git runs none of these after `git reset`, `git cherry-pick`, `git revert`, `git stash`, `git bisect`, `git submodule update` or a `git rebase --abort`, which may change the pins or move the submodules as well; nor after `git clone`, before the installer has run. `githook-submodule-roll roll`, run by hand in the parent, rolls forward the submodules that are at the pins of the index after such a command. A rebase that is aborted after it stopped leaves each submodule at the target of the commit it started from, which no longer follows a pin of the parent; `git submodule update` and `githook-submodule-roll roll` then roll it forward from the pin.

The hook cannot undo the command that ran it, which has finished. A failure of the hook, such as a git command that it could not run, is printed; after a checkout, it also makes `git checkout` or `git switch` exit with status 1, whereas a merge or a rebase exits as it would have.

## Installation

The hook is written in Rust, a crate that is a cargo workspace of its own, so that a repository may keep it as a submodule apart from its own build. `install.sh`, run from inside a checkout, installs it with `cargo install`, which needs a Rust toolchain, into `bin/githook-submodule-roll` in the repository's common git directory, and registers it in the repository's config as `hook.submodule-roll-post-checkout`, `hook.submodule-roll-post-merge` and `hook.submodule-roll-post-rewrite`, which run beside any other hooks of the events. Configured hooks need a git that supports `hook.<name>.event`; the hook is tested with git 2.56. The commands name the binary through `$(git rev-parse --git-common-dir)`, which the shell resolves each time git runs the hook, so the repository may move. It may be run again to update the hook. Run from inside a submodule, it refuses, since it would install into the submodule's repository rather than the one that mounts it; `INSTALL_SUBMODULE=1` installs into the submodule nonetheless. It adds the refspec of the replaces notes to each remote of each submodule that is checked out, within submodules too, unless the remote has it already. A submodule that is checked out later lacks the refspec until the installer runs again. It then runs `githook-submodule-roll roll`, so that the submodules that are at their pins are rolled forward without waiting for a checkout. Each clone must run it once; worktrees share the hook and the config of their repository.
