//! Records the kit commit this generator is built from, so new apps depend on
//! the same mhr_gpui_kit their template was written against.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let rev = kit_rev(&manifest_dir).unwrap_or_default();
    if rev.is_empty() {
        println!(
            "cargo:warning=no mhr_gpui_kit git commit found, new apps will follow the kit's main branch"
        );
    }
    println!("cargo:rustc-env=MHR_GPUI_NEW_KIT_REV={rev}");
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
