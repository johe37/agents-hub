#!/usr/bin/env bash
# Bump the version in Cargo.toml, commit and tag with release notes by Claude. Push is left to the caller.
set -euo pipefail

usage() {
    echo "usage: $0 <version>   (e.g. 0.6.0)" >&2
    exit 1
}

[[ $# -eq 1 ]] || usage
version=$1
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || usage

cd "$(git rev-parse --show-toplevel)"

if [[ -n $(git status --porcelain) ]]; then
    echo "error: working tree is not clean" >&2
    exit 1
fi

branch=$(git branch --show-current)
if [[ $branch != master ]]; then
    echo "error: on branch '$branch', expected master" >&2
    exit 1
fi

if git rev-parse -q --verify "refs/tags/$version" >/dev/null; then
    echo "error: tag $version already exists" >&2
    exit 1
fi

previous=$(git describe --tags --abbrev=0 2>/dev/null || true)
log=$(git log --no-merges --format='- %s%n%w(0,2,2)%b' ${previous:+"$previous.."}HEAD)
prompt="Write release notes for claude-hub $version, a TUI that manages Claude Code sessions \
in tmux, from the commit messages on stdin. Write for users: group the changes under the \
plain-text headings New, Changed and Fixed (leave out empty ones), one short bullet each, \
merge commits that belong together, and skip internal changes such as refactors and docs. \
No title, no intro, no Markdown headings. Output only the notes."
# No hooks, so this one-off session does not show up in the hub.
if ! notes=$(printf '%s\n' "$log" | claude -p --tools "" --strict-mcp-config --settings '{"disableAllHooks": true}' "$prompt") || [[ -z $notes ]]; then
    echo "warning: claude could not summarize the release; using the commit subjects" >&2
    notes=$(git log --no-merges --format='- %s' ${previous:+"$previous.."}HEAD)
fi

# Only the [package] version line sits at column 0; dependency versions are inline.
sed -i "0,/^version = \".*\"/s//version = \"$version\"/" Cargo.toml
cargo check --quiet

git add Cargo.toml Cargo.lock
git commit -m "Release $version"
git tag -a --cleanup=verbatim "$version" -m "$version" -m "$notes"

echo
echo "$notes"
echo
echo "Tagged $version with the notes above. Push with:"
echo "  git push origin master $version"
echo "The tag push creates the GitHub release with these notes."
