#!/usr/bin/env bash
# Installs the submodule roll into the repository this is run from: cargo
# install puts the binary in bin/ in its common git directory, and the
# repository's config registers it as a post-checkout and a post-merge hook, so
# that it runs beside any other hooks of the events. The command finds the git
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
echo "githook-submodule-roll: installed into $root/bin"
# The submodules are rolled forward from the pins of the index, as the next
# checkout would roll them.
"$root/bin/githook-submodule-roll" roll ||
  echo "githook-submodule-roll: the hook is installed, but could not roll the submodules forward" >&2
