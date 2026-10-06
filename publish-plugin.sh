#!/usr/bin/env bash
# Release a plugin: set its version in its manifest (and its crate, for a driver),
# commit, tag plugins/<id>-v<version> and push. The tag runs
# .github/workflows/plugins-release.yml, which builds, publishes and signs the index.
#
#   ./publish-plugin.sh <id> <version> [--dry-run]
set -euo pipefail
cd "$(dirname "$0")"

die() {
  printf 'publish-plugin: %s\n' "$*" >&2
  exit 1
}

id=${1:-} version=${2:-}
dry_run=false
[[ ${3:-} == --dry-run ]] && dry_run=true
[[ -n $id && -n $version ]] || die "usage: ./publish-plugin.sh <id> <version> [--dry-run]"
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || die "not a version: $version"
manifest=plugins/$id/dexo-plugin.toml
[[ -f $manifest ]] || die "no $manifest"
crate=crates/dexo-driver-$id/Cargo.toml
driver=false
grep -qx 'kind = "driver"' "$manifest" && driver=true
if $driver && [[ ! -f $crate ]]; then
  die "a driver plugin is built from $crate, which is missing"
fi

[[ $(git branch --show-current) == main ]] || die "plugins are released from main"
[[ -z $(git status --porcelain --untracked-files=no) ]] || die "the working tree has uncommitted changes"
git fetch --quiet --tags origin
[[ $(git rev-parse HEAD) == "$(git rev-parse origin/main)" ]] ||
  die "main is not in sync with origin/main; pull or push first"
tag=plugins/$id-v$version
git rev-parse -q --verify "refs/tags/$tag" >/dev/null && die "tag $tag already exists"

if $dry_run; then
  echo "would set $id to $version, commit, and push $tag"
  exit 0
fi

# The first `version = ` line only: the package's, not a dependency's.
set_version() { perl -0pi -e 's/^version = "[^"]*"/version = "'"$version"'"/m' "$1"; }
set_version "$manifest"
files=("$manifest")
if $driver; then
  set_version "$crate"
  cargo metadata --format-version 1 >/dev/null # brings Cargo.lock along
  files+=("$crate" Cargo.lock)
fi
git commit -m "chore(plugins): $id $version" -- "${files[@]}"
git tag -a "$tag" -m "Plugin $id $version"
git push origin main "$tag"
