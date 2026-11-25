//! One-time move from the old name, claude-hub, to jugglr.
//!
//! Each old path becomes a symlink to the new one, so a running claude-hub engine, its agents
//! (whose $TMUX names the old socket), old hooks and git worktrees under the cache keep working.

use std::path::{Path, PathBuf};

use crate::config::{cache_dir, config_dir, state_dir};

const OLD: &str = "claude-hub";

pub fn run() {
    for new in [state_dir(), cache_dir(), config_dir()] {
        if let Some(parent) = new.parent() {
            move_and_link(&parent.join(OLD), &new);
        }
    }
    let socket = PathBuf::from(crate::tmux::socket_path());
    if let Some(dir) = socket.parent() {
        move_and_link(&dir.join(OLD), &dir.join(crate::tmux::SOCKET_NAME));
    }
}

fn move_and_link(old: &Path, new: &Path) {
    let Ok(meta) = std::fs::symlink_metadata(old) else { return };
    if meta.file_type().is_symlink() || std::fs::symlink_metadata(new).is_ok() {
        return;
    }
    if std::fs::rename(old, new).is_ok() {
        let _ = std::os::unix::fs::symlink(new, old);
    }
}
