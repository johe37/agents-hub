//! Builds the web app (web/dist) that `jugglr web` serves from inside the binary.
//! Without npm, the binary still builds; `jugglr web` then says the app is missing.

use std::path::Path;
use std::process::Command;

fn npm(args: &[&str]) -> bool {
    Command::new("npm").args(args).current_dir("web").status().is_ok_and(|s| s.success())
}

fn main() {
    for p in ["web/src", "web/public", "web/index.html", "web/package.json", "web/package-lock.json", "web/vite.config.ts"] {
        println!("cargo:rerun-if-changed={p}");
    }
    println!("cargo:rerun-if-env-changed=JUGGLR_SKIP_WEB");
    let dist = Path::new("web/dist");
    if std::env::var_os("JUGGLR_SKIP_WEB").is_some() && dist.join("index.html").exists() {
        return;
    }
    let installed = Path::new("web/node_modules").exists() || npm(&["ci", "--no-audit", "--no-fund"]);
    if installed && npm(&["run", "build"]) {
        return;
    }
    println!("cargo:warning=Could not build the web app with npm; `jugglr web` will not work in this build.");
    if !dist.join("index.html").exists() {
        let _ = std::fs::create_dir_all(dist);
        let _ = std::fs::write(dist.join("index.html"), "<!doctype html><title>jugglr</title><p>This jugglr was built without the web app. Install Node.js and npm, then build again.</p>");
    }
}
