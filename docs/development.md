# Development

## Tests

`cargo test` runs the tests. A GitHub workflow runs them on every push to master and on pull
requests.

## Demo

`scripts/demo.sh` starts a hub with made-up workspaces, agents and merge requests, to try a
change by hand. It runs on its own tmux server with its own HOME, so your real hub is not
touched. Run it outside tmux and it attaches at once. `scripts/demo.sh stop` removes it.

## Release

`scripts/release.sh 0.6.0` sets the version in `Cargo.toml`, commits and tags. Claude writes
release notes from the commits since the last tag and the tag keeps them.
Then push with `git push origin master 0.6.0`. The push of the tag starts a GitHub workflow
that creates the GitHub release with these notes.
