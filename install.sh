#!/usr/bin/env bash
# Installs the submodule roll into the repository this is run from: cargo
# install puts the binary in bin/ in its common git directory, and the
# repository's config registers it as a post-checkout and a post-merge hook, so
# that it runs beside any other hooks of the events, and each submodule's
# remotes fetch their replaces notes. The command finds the git
# directory each time it runs, so the repository may move. Running this again
# updates the hook.
set -euo pipefail
shopt -s lastpipe
IFS=$'\n\t'
if [ -n "$(git rev-parse --show-superproject-working-tree)" ] && [ "${INSTALL_SUBMODULE:-}" != 1 ]; then
  echo "githook-submodule-roll: $(git rev-parse --show-toplevel) is a submodule; run this from the repository that mounts it, or set INSTALL_SUBMODULE=1 to install into the submodule itself" >&2
  exit 1
fi
src=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &>/dev/null && pwd -P)
root=$(git rev-parse --path-format=absolute --git-common-dir)

cargo install --locked --no-track --force --quiet --root "$root" --path "$src"
# shellcheck disable=SC2016
bin='"$(git rev-parse --git-common-dir)"/bin'
for event in post-checkout post-merge post-rewrite; do
  git config "hook.submodule-roll-$event.event" "$event"
  git config "hook.submodule-roll-$event.command" "$bin/githook-submodule-roll $event"
done
# The replaces notes of each submodule's remotes come in with its fetches. The
# refspec is a glob so that a remote without them matches nothing.
# shellcheck disable=SC2016
git submodule foreach --quiet --recursive '
  for remote in $(git remote); do
    spec="+refs/notes/replaces*:refs/notes/remotes/$remote/replaces*"
    git config --get-all "remote.$remote.fetch" | grep -qxF -- "$spec" ||
      git config --add "remote.$remote.fetch" "$spec"
  done
'
# When this checkout is a submodule of the repository, the submodule roll rolls
# it forward to its latest compatible release, unless the repository's
# attributes say otherwise.
top=$(cd -- "$(git rev-parse --show-toplevel)" && pwd -P)
super=$(git -C "$src" rev-parse --show-superproject-working-tree 2>/dev/null || true)
if [ -n "$super" ] && [ "$(cd -- "$super" && pwd -P)" = "$top" ]; then
  path=${src#"$top"/}
  attributes=$top/.gitattributes
  if [ "$(git -C "$top" check-attr submodule-roll -- "$path")" = "$path: submodule-roll: unspecified" ]; then
    if [ -s "$attributes" ] && [ -n "$(tail -c1 "$attributes")" ]; then
      echo >>"$attributes"
    fi
    printf '/%s submodule-roll\n' "$path" >>"$attributes"
    echo "githook-submodule-roll: set submodule-roll on $path in .gitattributes; commit it"
  fi
fi
echo "githook-submodule-roll: installed into $root/bin"
# The submodules are rolled forward from the pins of the index, as the next
# checkout would roll them.
"$root/bin/githook-submodule-roll" roll ||
  echo "githook-submodule-roll: the hook is installed, but could not roll the submodules forward" >&2
