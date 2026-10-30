//! Records the version of the kit this generator is built with, so new apps depend on the
//! release their template was written against.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let kit_manifest = manifest_dir.join("../kit/Cargo.toml");
    println!("cargo:rerun-if-changed={}", kit_manifest.display());
    let text = fs::read_to_string(&kit_manifest)
        .unwrap_or_else(|err| panic!("can't read {}: {err}", kit_manifest.display()));
    // The first `version` of the manifest is the package's, in `[package]`.
    let version = text
        .lines()
        .find_map(|line| line.strip_prefix("version = \"")?.strip_suffix('"'))
        .unwrap_or_else(|| panic!("{} has no version", kit_manifest.display()));
    println!("cargo:rustc-env=GPUI_NEW_KIT_VERSION={version}");
}
