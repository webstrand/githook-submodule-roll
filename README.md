# githook-submodule-roll

A git submodule is pinned to one exact commit. When the submodule publishes a release with a fix, every repository that mounts it keeps the old commit until someone moves the pin by hand. And when the submodule's history is rewritten, the pinned commit may vanish from its remote, so that the pin cannot be checked out at all.

This hook treats a pin as the oldest release that the parent works with. After each checkout, merge or rebase, it moves each submodule that opts in to the newest release tag that semver says is compatible with its pin. A pin at or after `v1.2.3` thus runs the newest `v1.x.y`, and a pin at or after `v0.3.1` the newest `v0.3.y`. The parent's pins are never changed; `git status` shows a rolled submodule as modified until the new commit is staged.

A submodule opts in through the `submodule-roll` attribute on its path in `.gitattributes`:

```
/tools/lib submodule-roll
```

A submodule without the attribute is left at whatever commit it is at, since its tags need not follow semver.

## Requirements

- A Rust toolchain, for `cargo install`.
- A git that supports `hook.<name>.event`. The hook is tested with git 2.56.
- bash 4.2 or later, for `install.sh`.

## Installation

The hook is installed into a repository by running `install.sh` from a checkout of that repository, usually with githook-submodule-roll mounted in it as a submodule:

```sh
git submodule add https://github.com/webstrand/githook-submodule-roll.git tools/submodule-roll
tools/submodule-roll/install.sh
```

The script installs the hook into `bin/` in the repository's common git directory and registers it for `post-checkout`, `post-merge` and `post-rewrite`, beside any other hooks of those events. When run from a submodule of the repository, it sets `submodule-roll` on its own path in `.gitattributes`, which the operator commits. Each clone must run it once; worktrees share the hook and the config of their repository. Running it again updates the hook.

Only the tags that a submodule's repository already holds are read; the hook does not fetch. `git -C <submodule> fetch --tags` brings in the tags of later releases.

## Rewritten pins

When a submodule's remote rewrites its history, no later tag contains the pinned commit. Where the remote keeps the replaces notes of [githook-replaces-index](https://github.com/webstrand/githook-replaces-index), the hook follows them from the pin to the commits that stand in its place, and rolls forward to a tag that holds one of them. `install.sh` adds the refspec that fetches those notes to each remote of each checked-out submodule.

A roll forward that relies on the notes prints a warning, since the notes are a ref that a remote may not keep. `githook-submodule-roll rewrite-pins` rewrites the parent's history so that its pins name the replacement commits directly.

## Commands

The binary is installed as `"$(git rev-parse --git-common-dir)"/bin/githook-submodule-roll`, outside `PATH`.

| Command | Role |
| - | - |
| `githook-submodule-roll roll` | rolls forward the submodules at the pins of the index, after a command that runs no hook, such as `git reset` or `git submodule update` |
| `githook-submodule-roll rewrite-pins [--dry-run] [<branch>...]` | rewrites the history of the branches so that each pin replaced by a rewrite names its replacement; requires githook-replaces-index's `post-rewrite` hook |

[HOOK.md](HOOK.md) is the full reference.
