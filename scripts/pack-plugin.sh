#!/usr/bin/env bash
# Pack a plugin's .tar.gz from plugins/<id>: its manifest, the licence and its README,
# and the driver's executable under bin/ or the theme files under themes/.
#
#   scripts/pack-plugin.sh <id> <version> <target> [executable]
#
# A driver passes its target triple and executable; a theme pack passes `any`. The
# archive lands in dist/; its name, sha256 and size are printed.
set -euo pipefail
cd "$(dirname "$0")/.."

die() {
  printf 'pack-plugin: %s\n' "$*" >&2
  exit 1
}

id=${1:-} version=${2:-} target=${3:-} executable=${4:-}
[[ -n $id && -n $version && -n $target ]] ||
  die "usage: scripts/pack-plugin.sh <id> <version> <target|any> [executable]"
source_dir=plugins/$id
[[ -f $source_dir/dexo-plugin.toml ]] || die "no $source_dir/dexo-plugin.toml"
grep -qx "version = \"$version\"" "$source_dir/dexo-plugin.toml" ||
  die "$source_dir/dexo-plugin.toml is not at $version"

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
cp "$source_dir/dexo-plugin.toml" "$stage/"
cat LICENSE-MIT <(printf '\n') LICENSE-APACHE >"$stage/LICENSE"
[[ -f $source_dir/README.md ]] && cp "$source_dir/README.md" "$stage/"
if [[ -n $executable ]]; then
  mkdir "$stage/bin"
  cp "$executable" "$stage/bin/"
  name=$id-$version-$target.tar.gz
else
  [[ $target == any ]] || die "a theme pack's target is any"
  mkdir "$stage/themes"
  cp "$source_dir"/themes/*.toml "$stage/themes/"
  name=$id-$version.tar.gz
fi
mkdir -p dist
# Entries named plainly -- no `./` -- and no macOS resource forks: Dexo refuses any
# file a plugin does not ship.
(cd "$stage" && COPYFILE_DISABLE=1 tar -czf - -- *) >"dist/$name"
sha=$( (sha256sum "dist/$name" 2>/dev/null || shasum -a 256 "dist/$name") | cut -d' ' -f1)
size=$(wc -c <"dist/$name" | tr -d ' ')
printf '%s %s %s\n' "$name" "$sha" "$size"
