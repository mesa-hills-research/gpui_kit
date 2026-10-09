use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use gpui_new::{Contents, FILES, Project, generate};

/// A fresh, missing directory named `name` for one test.
fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "gpui_new-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    base.join(name)
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gpui_new"))
        .args(args)
        .output()
        .unwrap()
}

fn run_ok(args: &[&str]) {
    let output = run(args);
    assert!(
        output.status.success(),
        "gpui_new {args:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn read_manifest(dir: &Path) -> toml::Table {
    let text = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    text.parse::<toml::Table>()
        .unwrap_or_else(|err| panic!("Cargo.toml doesn't parse: {err}\n{text}"))
}

/// Every `{{name}}` left in `text`.
fn placeholders(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let name = &after[..end];
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    found.push(name.to_string());
                }
                rest = &after[end + 2..];
            }
            None => break,
        }
    }
    found
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(files_under(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
fn generates_every_file_with_no_placeholder_left() {
    let dir = scratch("my-app");
    run_ok(&[dir.to_str().unwrap(), "--no-git"]);

    let project = Project::new(&dir, None, None).unwrap();
    let mut expected: Vec<PathBuf> = FILES
        .iter()
        .map(|file| dir.join(project.substitute(file.output)))
        .collect();
    let mut written = files_under(&dir);
    expected.sort();
    written.sort();
    assert_eq!(written, expected);

    for path in &written {
        let Ok(text) = fs::read_to_string(path) else {
            continue; // the icons
        };
        assert_eq!(
            placeholders(&text),
            Vec::<String>::new(),
            "in {}",
            path.display()
        );
    }
    assert!(
        dir.join("packaging/linux/com.example.MyApp.desktop")
            .is_file()
    );
    assert!(!dir.join(".git").exists());
}

#[test]
fn the_template_has_no_unknown_placeholders() {
    let dir = scratch("check");
    let project = Project::new(&dir, None, None).unwrap();
    for file in FILES {
        if let Contents::Text(text) = file.contents {
            let left = placeholders(&project.substitute(text));
            assert!(
                left.is_empty(),
                "{} has unknown placeholders {left:?}",
                file.source
            );
        }
        assert!(placeholders(&project.substitute(file.output)).is_empty());
    }
}

#[test]
fn every_template_file_is_embedded() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("template");
    let mut on_disk: Vec<String> = files_under(&root)
        .iter()
        .map(|path| {
            let relative = path.strip_prefix(&root).unwrap();
            relative.to_str().unwrap().replace('\\', "/")
        })
        .collect();
    let mut embedded: Vec<String> = FILES.iter().map(|file| file.source.to_string()).collect();
    on_disk.sort();
    embedded.sort();
    assert_eq!(on_disk, embedded);
}

#[test]
fn the_manifest_parses_and_names_the_crate() {
    for (dir_name, crate_name) in [
        ("my-app", "my-app"),
        ("my_app", "my_app"),
        ("My Cool App", "my-cool-app"),
        ("Notes.v2", "notes-v2"),
    ] {
        let dir = scratch(dir_name);
        run_ok(&[dir.to_str().unwrap(), "--no-git"]);
        let manifest = read_manifest(&dir);
        let package = manifest["package"].as_table().unwrap();
        assert_eq!(package["name"].as_str(), Some(crate_name));
        assert_eq!(package["edition"].as_str(), Some("2024"));

        let kit = manifest["dependencies"]["gpui-kit"].as_table().unwrap();
        assert_eq!(kit["git"].as_str(), Some(gpui_new::KIT_GIT));
        match (kit.get("rev"), kit.get("branch")) {
            (Some(rev), None) => assert_eq!(rev.as_str(), Some(gpui_new::KIT_REV)),
            (None, Some(branch)) => {
                assert!(gpui_new::KIT_REV.is_empty());
                assert_eq!(branch.as_str(), Some("main"));
            }
            other => panic!("gpui-kit is pinned by {other:?}"),
        }
        assert!(manifest["build-dependencies"].get("winresource").is_some());
    }
}

/// New apps patch the gpui crates to the fork at the commit the kit's own
/// `[patch.crates-io]` names, since Cargo applies only the app's patches.
#[test]
fn the_manifest_patches_gpui_to_the_kits_fork() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let text = fs::read_to_string(&workspace).unwrap();
    let workspace = text.parse::<toml::Table>().unwrap();
    let kit_patch = workspace["patch"]["crates-io"].as_table().unwrap();
    assert_eq!(
        kit_patch["gpui-pre"]["rev"].as_str(),
        Some(gpui_new::GPUI_REV)
    );

    let dir = scratch("patched");
    run_ok(&[dir.to_str().unwrap(), "--no-git"]);
    let manifest = read_manifest(&dir);
    let patch = manifest["patch"]["crates-io"].as_table().unwrap();
    assert!(patch.contains_key("gpui-pre"));
    for (name, entry) in patch {
        let kit_entry = kit_patch
            .get(name)
            .unwrap_or_else(|| panic!("the kit doesn't patch {name}"));
        assert_eq!(entry["git"].as_str(), Some(gpui_new::GPUI_GIT), "{name}");
        assert_eq!(entry["git"].as_str(), kit_entry["git"].as_str(), "{name}");
        assert_eq!(entry["rev"].as_str(), kit_entry["rev"].as_str(), "{name}");
    }
}

#[test]
fn cargo_fetches_the_kit_with_the_git_cli() {
    let dir = scratch("fetch");
    run_ok(&[dir.to_str().unwrap(), "--no-git"]);
    let text = fs::read_to_string(dir.join(".cargo/config.toml")).unwrap();
    let config = text
        .parse::<toml::Table>()
        .unwrap_or_else(|err| panic!(".cargo/config.toml doesn't parse: {err}\n{text}"));
    assert_eq!(config["net"]["git-fetch-with-cli"].as_bool(), Some(true));
}

#[test]
fn name_and_app_id_flow_into_the_app() {
    let dir = scratch("notes");
    run_ok(&[
        dir.to_str().unwrap(),
        "--name",
        "Field Notes",
        "--app-id=org.example.FieldNotes",
        "--no-git",
    ]);
    let main = fs::read_to_string(dir.join("src/main.rs")).unwrap();
    assert!(main.contains(r#"pub const APP_NAME: &str = "Field Notes";"#));
    assert!(main.contains(r#"pub const APP_ID: &str = "org.example.FieldNotes";"#));
    let desktop =
        fs::read_to_string(dir.join("packaging/linux/org.example.FieldNotes.desktop")).unwrap();
    assert!(desktop.lines().any(|line| line == "Name=Field Notes"));
    assert!(desktop.lines().any(|line| line == "Exec=notes"));
    assert!(
        dir.join("packaging/linux/org.example.FieldNotes.png")
            .is_file()
    );
    assert_eq!(
        read_manifest(&dir)["package"]["description"].as_str(),
        Some("Field Notes")
    );
}

#[test]
fn a_non_empty_directory_is_refused() {
    let dir = scratch("taken");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("notes.txt"), "keep me").unwrap();

    let output = run(&[dir.to_str().unwrap(), "--no-git"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("is not empty"));
    assert_eq!(files_under(&dir), vec![dir.join("notes.txt")]);
    assert_eq!(
        fs::read_to_string(dir.join("notes.txt")).unwrap(),
        "keep me"
    );

    let project = Project::new(&dir, None, None).unwrap();
    assert!(generate(&dir, &project).is_err());
}

#[test]
fn an_empty_directory_is_used() {
    let dir = scratch("empty");
    fs::create_dir_all(&dir).unwrap();
    run_ok(&[dir.to_str().unwrap(), "--no-git"]);
    assert!(dir.join("Cargo.toml").is_file());
}

#[test]
fn a_file_in_the_way_is_refused() {
    let path = scratch("file");
    fs::write(&path, "").unwrap();
    let output = run(&[path.to_str().unwrap(), "--no-git"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("is not a directory"));
}

#[test]
fn bad_names_and_arguments_are_refused() {
    let base = scratch("unused");
    let cases: &[&[&str]] = &[
        &["--no-git"],
        &["2048-game"],
        &["fn"],
        &["ok", "--app-id", "MyApp"],
        &["ok", "--name", "Say \"hi\""],
        &["ok", "--name"],
        &["ok", "--colour"],
        &["ok", "extra"],
    ];
    for args in cases {
        let mut args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        if !args[0].starts_with('-') {
            args[0] = base.with_file_name(&args[0]).to_str().unwrap().to_string();
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = run(&args);
        assert!(!output.status.success(), "{args:?} succeeded");
        assert!(!base.with_file_name("ok").exists(), "{args:?} wrote files");
    }
}

#[test]
fn starts_a_git_repository_when_git_is_installed() {
    let git = |dir: &Path, args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .ok()
    };
    let dir = scratch("with-git");
    let parent = dir.parent().unwrap();
    let Some(inside) = git(parent, &["rev-parse", "--is-inside-work-tree"]) else {
        return; // Git isn't installed.
    };
    run_ok(&[dir.to_str().unwrap()]);
    // Inside an existing repository the new app joins it instead.
    assert_eq!(dir.join(".git").is_dir(), !inside.status.success());
}

#[test]
fn help_and_version() {
    for flag in ["--help", "-h"] {
        let output = run(&[flag]);
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: gpui_new"));
    }
    let output = run(&["--version"]);
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("gpui_new "));
}
