#!/usr/bin/env bash
# Cut a Dexo release: bump the workspace version, write the CHANGELOG section,
# commit, tag, and push. The tag triggers .github/workflows/release.yml.
#
#   ./publish.sh patch|minor|major        bump from the current version
#   ./publish.sh 1.2.0 | 1.2.0-rc.1       exact version (a prerelease skips the
#                                         package managers and the CHANGELOG)
#   ./publish.sh minor --dry-run          show what would happen, change nothing
set -euo pipefail
cd "$(dirname "$0")"

die() {
  printf 'publish: %s\n' "$*" >&2
  exit 1
}

bump=${1:-}
dry_run=false
[[ ${2:-} == --dry-run ]] && dry_run=true
[[ -n $bump ]] || die "usage: ./publish.sh <patch|minor|major|X.Y.Z[-pre]> [--dry-run]"

[[ $(git branch --show-current) == main ]] || die "releases are cut from main"
[[ -z $(git status --porcelain) ]] || die "the working tree has uncommitted changes"
git fetch --quiet --tags origin
[[ $(git rev-parse HEAD) == "$(git rev-parse origin/main)" ]] ||
  die "main is not in sync with origin/main; pull or push first"

semver='^([0-9]+)\.([0-9]+)\.([0-9]+)(-[0-9A-Za-z.-]+)?$'
current=$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' Cargo.toml)
[[ $current =~ $semver ]] || die "cannot read the workspace version from Cargo.toml"
major=${BASH_REMATCH[1]} minor=${BASH_REMATCH[2]} patch=${BASH_REMATCH[3]} pre=${BASH_REMATCH[4]}

# From a prerelease, a bump finishes that release first (1.2.0-rc.1 -> minor -> 1.2.0).
case $bump in
  patch) [[ -n $pre ]] && next="$major.$minor.$patch" || next="$major.$minor.$((patch + 1))" ;;
  minor) [[ -n $pre && $patch == 0 ]] && next="$major.$minor.0" || next="$major.$((minor + 1)).0" ;;
  major) [[ -n $pre && $minor == 0 && $patch == 0 ]] && next="$major.0.0" || next="$((major + 1)).0.0" ;;
  *)
    [[ $bump =~ $semver ]] || die "not a version or bump: $bump"
    next=$bump
    ;;
esac

# Semver order: the numeric core first, then a release outranks its prereleases.
newer() {
  local a=${1%%-*} b=${2%%-*}
  if [[ $a != "$b" ]]; then
    [[ $(printf '%s\n%s\n' "$a" "$b" | sort -V | tail -1) == "$a" ]]
    return
  fi
  [[ $1 == "$2" ]] && return 1
  [[ $1 != *-* ]] && return 0
  [[ $2 != *-* ]] && return 1
  [[ $(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -1) == "$1" ]]
}
newer "$next" "$current" || die "$next is not newer than $current"
git rev-parse -q --verify "refs/tags/v$next" >/dev/null && die "tag v$next already exists"

section=""
if [[ $next != *-* ]]; then
  since=$(git tag --list 'v[0-9]*' --sort=-v:refname | grep -v -- - | head -1 || true)
  features=() fixes=() perf=() other=()
  while IFS= read -r subject; do
    if [[ $subject =~ ^([a-z]+)(\([^\)]*\))?!?:\ (.+)$ ]]; then
      type=${BASH_REMATCH[1]} text=${BASH_REMATCH[3]}
    else
      type=other text=$subject
    fi
    case $type in
      feat) features+=("${text^}") ;;
      fix) fixes+=("${text^}") ;;
      perf) perf+=("${text^}") ;;
      other) other+=("${text^}") ;;
    esac
  done < <(git log --no-merges --format=%s "${since:+$since..}HEAD")

  group() {
    local title=$1
    shift
    (($# > 0)) || return 0
    printf '### %s\n\n' "$title"
    printf -- '- %s\n' "$@"
    printf '\n'
  }
  section=$(
    printf '## %s\n\n' "$next"
    group Features "${features[@]}"
    group Fixes "${fixes[@]}"
    group Performance "${perf[@]}"
    group "Other changes" "${other[@]}"
  )
  [[ $section == *"### "* ]] || section=$(printf '## %s\n\nMaintenance release.\n' "$next")
fi

printf 'Dexo %s -> %s\n\n' "$current" "$next"
if [[ -n $section ]]; then
  printf '%s\n\n' "$section"
else
  printf 'Prerelease: no CHANGELOG entry, and the package managers are skipped.\n\n'
fi
$dry_run && {
  echo "dry run: nothing changed"
  exit 0
}

read -r -p "Publish v$next? [y/N] " answer
[[ $answer == y ]] || die "aborted; nothing changed"

sed -i "/^\[workspace\.package\]/,/^\[/s/^version = \"$current\"$/version = \"$next\"/" Cargo.toml
cargo update --workspace --offline --quiet 2>/dev/null || cargo update --workspace --quiet
files=(Cargo.toml Cargo.lock)
if [[ -n $section ]]; then
  [[ $(head -1 CHANGELOG.md) == "# Changelog" ]] || die "CHANGELOG.md must start with '# Changelog'"
  { printf '# Changelog\n\n%s\n\n' "$section"; tail -n +3 CHANGELOG.md; } >CHANGELOG.md.next
  mv CHANGELOG.md.next CHANGELOG.md
  files+=(CHANGELOG.md)
fi

git add "${files[@]}"
git commit --quiet -m "chore(release): v$next"
git tag -a "v$next" -m "Dexo $next"
git push --atomic --quiet origin main "v$next"

repo=$(git remote get-url origin | sed -E 's#(git@github\.com:|https://github\.com/)##; s#\.git$##')
echo "pushed v$next: https://github.com/$repo/actions/workflows/release.yml"
