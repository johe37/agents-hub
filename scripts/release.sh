#!/usr/bin/env bash
# Bump the version in Cargo.toml, commit and tag. Push is left to the caller.
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

# Only the [package] version line sits at column 0; dependency versions are inline.
sed -i "0,/^version = \".*\"/s//version = \"$version\"/" Cargo.toml
cargo check --quiet

git add Cargo.toml Cargo.lock
git commit -m "Release $version"
git tag -a "$version" -m "$version"

echo
echo "Tagged $version. Push with:"
echo "  git push origin master $version"
