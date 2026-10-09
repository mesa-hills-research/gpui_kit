//! Records the kit commit this generator is built from, so new apps depend on
//! the same kit their template was written against, and the gpui fork commit
//! that kit builds with.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let rev = kit_rev(&manifest_dir).unwrap_or_default();
    if rev.is_empty() {
        println!(
            "cargo:warning=no git commit of the kit found, new apps will follow the kit's main branch"
        );
    }
    println!("cargo:rustc-env=GPUI_NEW_KIT_REV={rev}");

    let gpui_rev = gpui_rev(&manifest_dir).unwrap_or_default();
    if gpui_rev.is_empty() {
        println!(
            "cargo:warning=no gpui fork commit found in the kit's Cargo.toml, new apps will follow the fork's main branch"
        );
    }
    println!("cargo:rustc-env=GPUI_NEW_GPUI_REV={gpui_rev}");
}

/// The rev of the gpui fork in the workspace's `[patch.crates-io]`, read from
/// its `gpui-pre` entry.
fn gpui_rev(dir: &Path) -> Option<String> {
    let manifest = dir.join("../../Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(manifest).ok()?;
    let line = text.lines().find(|line| {
        line.split('=').next().map(str::trim) == Some("gpui-pre")
            && line.contains("\"https://github.com/mesa-hills-research/gpui\"")
    })?;
    let rev = line.split("rev = \"").nth(1)?.split('"').next()?;
    (rev.len() == 40 && rev.bytes().all(|b| b.is_ascii_hexdigit())).then(|| rev.to_string())
}

fn kit_rev(dir: &Path) -> Option<String> {
    // Only a checkout of the kit itself names a usable commit.
    if !dir.join("../kit/Cargo.toml").is_file() {
        return None;
    }
    let git = |args: &[&str]| -> Option<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .ok()?;
        let text = String::from_utf8(output.stdout).ok()?;
        output.status.success().then(|| text.trim().to_string())
    };
    let rev = git(&["rev-parse", "HEAD"])?;

    // Build again when HEAD moves: a new commit, a checkout or a rebase.
    let mut watched = vec![
        git(&["rev-parse", "--git-path", "HEAD"]),
        git(&["rev-parse", "--git-path", "packed-refs"]),
    ];
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        watched.push(git(&["rev-parse", "--git-path", &branch]));
    }
    for path in watched.into_iter().flatten() {
        let path = dir.join(path);
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    Some(rev)
}
