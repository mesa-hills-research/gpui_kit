//! Creates a GPUI app from the template in `template/`.
//!
//! Text files go through placeholder substitution (`{{crate_name}}`,
//! `{{display_name}}`, `{{app_id}}`, `{{year}}`, `{{kit_ref}}` and
//! `{{gpui_ref}}`). Binary files are copied as they are.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The repository new apps take GPUI Kit from.
pub const KIT_GIT: &str = "https://github.com/mesa-hills-research/gpui_kit";

/// The kit commit this generator was built from, empty when the build found
/// none.
pub const KIT_REV: &str = env!("GPUI_NEW_KIT_REV");

/// The repository of the gpui fork the kit builds with.
pub const GPUI_GIT: &str = "https://github.com/mesa-hills-research/gpui";

/// The gpui fork commit in the kit's `[patch.crates-io]`, empty when the build
/// found none.
pub const GPUI_REV: &str = env!("GPUI_NEW_GPUI_REV");

/// One file of the template.
pub struct TemplateFile {
    /// Path under `template/`.
    pub source: &'static str,
    /// Path in the new app, which may contain placeholders.
    pub output: &'static str,
    pub contents: Contents,
}

pub enum Contents {
    Text(&'static str),
    Binary(&'static [u8]),
}

macro_rules! text {
    ($source:literal) => {
        text!($source => $source)
    };
    ($source:literal => $output:literal) => {
        TemplateFile {
            source: $source,
            output: $output,
            contents: Contents::Text(include_str!(concat!("../template/", $source))),
        }
    };
}

macro_rules! binary {
    ($source:literal => $output:literal) => {
        TemplateFile {
            source: $source,
            output: $output,
            contents: Contents::Binary(include_bytes!(concat!("../template/", $source))),
        }
    };
}

/// Every file of the template. `Cargo.toml`, `.gitignore` and
/// `.cargo/config.toml` carry other names in `template/` so that Cargo and Git
/// leave the template alone.
pub const FILES: &[TemplateFile] = &[
    text!("Cargo.toml.tmpl" => "Cargo.toml"),
    text!("gitignore" => ".gitignore"),
    text!("cargo/config.toml" => ".cargo/config.toml"),
    text!("README.md"),
    text!("build.rs"),
    text!("src/main.rs"),
    text!("src/chrome.rs"),
    text!("src/menus.rs"),
    text!("src/title_bar.rs"),
    text!("src/workspace.rs"),
    text!("assets/icon.svg"),
    binary!("assets/icon.ico" => "assets/icon.ico"),
    text!("packaging/linux/app.desktop" => "packaging/linux/{{app_id}}.desktop"),
    binary!("packaging/linux/app.png" => "packaging/linux/{{app_id}}.png"),
];

/// The names a new app is created with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    /// The package and binary name, such as `my-app`.
    pub crate_name: String,
    /// The name people see, such as `My App`.
    pub display_name: String,
    /// The reverse-DNS application id, such as `com.example.MyApp`.
    pub app_id: String,
    pub year: i64,
}

impl Project {
    /// Names a project after its directory, with optional overrides for the
    /// display name and the app id.
    pub fn new(
        path: &Path,
        display_name: Option<&str>,
        app_id: Option<&str>,
    ) -> Result<Self, Error> {
        let dir_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error::new(format!("can't name an app after {}", path.display())))?;
        let crate_name = crate_name(dir_name)?;
        let display_name = match display_name {
            Some(name) => validate_display_name(name)?,
            None => default_display_name(&crate_name),
        };
        let app_id = match app_id {
            Some(id) => validate_app_id(id)?,
            None => default_app_id(&display_name, &crate_name),
        };
        Ok(Self {
            crate_name,
            display_name,
            app_id,
            year: current_year(),
        })
    }

    /// Replaces every placeholder in `text`.
    pub fn substitute(&self, text: &str) -> String {
        text.replace("{{crate_name}}", &self.crate_name)
            .replace("{{display_name}}", &self.display_name)
            .replace("{{app_id}}", &self.app_id)
            .replace("{{year}}", &self.year.to_string())
            .replace("{{kit_ref}}", &kit_ref())
            .replace("{{gpui_ref}}", &gpui_ref())
    }
}

/// How the new app's `Cargo.toml` pins GPUI Kit.
pub fn kit_ref() -> String {
    if KIT_REV.is_empty() {
        r#"branch = "main""#.to_string()
    } else {
        format!(r#"rev = "{KIT_REV}""#)
    }
}

/// How the new app's `[patch.crates-io]` pins the gpui fork.
pub fn gpui_ref() -> String {
    if GPUI_REV.is_empty() {
        r#"branch = "main""#.to_string()
    } else {
        format!(r#"rev = "{GPUI_REV}""#)
    }
}

/// Writes the template into `target`, which must be missing or empty.
pub fn generate(target: &Path, project: &Project) -> Result<Vec<PathBuf>, Error> {
    if target.exists() {
        if !target.is_dir() {
            return Err(Error::new(format!(
                "{} exists and is not a directory",
                target.display()
            )));
        }
        let mut entries = fs::read_dir(target).map_err(|err| Error::io(target, err))?;
        if entries.next().is_some() {
            return Err(Error::new(format!("{} is not empty", target.display())));
        }
    }

    let mut written = Vec::with_capacity(FILES.len());
    for file in FILES {
        let path = target.join(project.substitute(file.output));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| Error::io(parent, err))?;
        }
        let result = match file.contents {
            Contents::Text(text) => fs::write(&path, project.substitute(text)),
            Contents::Binary(bytes) => fs::write(&path, bytes),
        };
        result.map_err(|err| Error::io(&path, err))?;
        written.push(path);
    }
    Ok(written)
}

/// Turns a directory name into a Cargo package name: ASCII letters and
/// digits in lower case, with `-` and `_` kept and anything else becoming `-`.
pub fn crate_name(dir_name: &str) -> Result<String, Error> {
    let mut name = String::with_capacity(dir_name.len());
    for c in dir_name.chars() {
        let c = match c {
            'a'..='z' | '0'..='9' | '_' => c,
            'A'..='Z' => c.to_ascii_lowercase(),
            _ => '-',
        };
        if c == '-' && name.ends_with('-') {
            continue;
        }
        name.push(c);
    }
    let name = name.trim_matches(['-', '_']).to_string();

    let explain = |reason: &str| {
        Error::new(format!(
            "can't name a crate after the directory {dir_name:?}: {reason}"
        ))
    };
    if name.is_empty() {
        return Err(explain("it has no ASCII letters or digits"));
    }
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(explain("a crate name starts with a letter"));
    }
    let ident = name.replace('-', "_");
    if KEYWORDS.contains(&ident.as_str()) {
        return Err(explain(&format!("`{ident}` is a Rust keyword")));
    }
    if RESERVED.contains(&ident.as_str()) || RESERVED.contains(&name.as_str()) {
        return Err(explain(&format!(
            "`{name}` is reserved by Rust, Cargo, Windows or the app's dependencies"
        )));
    }
    Ok(name)
}

/// `my-app` becomes `My App`.
pub fn default_display_name(crate_name: &str) -> String {
    words(crate_name).collect::<Vec<_>>().join(" ")
}

/// `My App` becomes `com.example.MyApp`. A display name that doesn't fit an
/// app id, such as one with accents, gives way to the crate name.
pub fn default_app_id(display_name: &str, crate_name: &str) -> String {
    let fits = display_name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || " -_".contains(c));
    let last = words(display_name).collect::<String>();
    if fits && last.starts_with(|c: char| c.is_ascii_alphabetic()) {
        format!("com.example.{last}")
    } else {
        format!("com.example.{}", words(crate_name).collect::<String>())
    }
}

fn words(name: &str) -> impl Iterator<Item = String> + '_ {
    name.split([' ', '-', '_'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            let first = chars.next().map(|c| c.to_ascii_uppercase());
            first.into_iter().chain(chars).collect()
        })
}

/// The display name ends up in Rust and TOML strings, the desktop entry and
/// the README, so it stays on one line without quotes or backslashes.
pub fn validate_display_name(name: &str) -> Result<String, Error> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::new("the display name is empty"));
    }
    if name
        .chars()
        .any(|c| c.is_control() || c == '"' || c == '\\')
    {
        return Err(Error::new(format!(
            "the display name {name:?} can't contain quotes, backslashes or control characters"
        )));
    }
    Ok(name.to_string())
}

/// App ids are reverse-DNS names that work as a macOS bundle id, a Windows
/// AppUserModelID and a Linux desktop file name: two or more parts separated
/// by dots, each starting with a letter and holding ASCII letters, digits and
/// hyphens.
pub fn validate_app_id(id: &str) -> Result<String, Error> {
    let parts: Vec<&str> = id.split('.').collect();
    let valid_part = |part: &&str| {
        part.starts_with(|c: char| c.is_ascii_alphabetic())
            && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    if parts.len() < 2 || !parts.iter().all(valid_part) || id.len() > 127 {
        return Err(Error::new(format!(
            "{id:?} is not a usable app id. Use a reverse-DNS name such as com.example.MyApp: \
             two or more parts separated by dots, each starting with a letter and holding \
             ASCII letters, digits and hyphens"
        )));
    }
    Ok(id.to_string())
}

/// The current year in UTC.
pub fn current_year() -> i64 {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    year_of_day(seconds as i64 / 86_400)
}

/// The proleptic Gregorian year of a day counted from 1970-01-01, after
/// Howard Hinnant's `civil_from_days`.
pub fn year_of_day(days: i64) -> i64 {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let year = year_of_era + era * 400;
    // The era starts in March, so January and February belong to the next year.
    if month_index >= 10 { year + 1 } else { year }
}

const KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "try", "type", "typeof",
    "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

#[rustfmt::skip]
const RESERVED: &[&str] = &[
    // Rust's own crates.
    "alloc", "core", "proc_macro", "std", "test",
    // Cargo's build directories.
    "build", "deps", "examples", "incremental",
    // Names Windows reserves for devices.
    "aux", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "con", "lpt1",
    "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9", "nul", "prn",
    // The app's own dependencies.
    "gpui", "gpui_kit", "gpui-kit", "tracing", "tracing_subscriber", "tracing-subscriber",
    "winresource",
];

/// A generator error, printed for the user.
#[derive(Debug)]
pub struct Error(String);

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    fn io(path: &Path, err: io::Error) -> Self {
        Self(format!("{}: {err}", path.display()))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_names_follow_cargo() {
        assert_eq!(crate_name("my-app").unwrap(), "my-app");
        assert_eq!(crate_name("my_app").unwrap(), "my_app");
        assert_eq!(crate_name("My App").unwrap(), "my-app");
        assert_eq!(crate_name("  Notes.app ").unwrap(), "notes-app");
        assert_eq!(crate_name("-tool-").unwrap(), "tool");
        assert_eq!(crate_name("a--b").unwrap(), "a-b");
        assert_eq!(crate_name("café").unwrap(), "caf");
        assert_eq!(crate_name("app2").unwrap(), "app2");
    }

    #[test]
    fn unusable_crate_names_are_refused() {
        for name in [
            "", "---", "2048", "match", "self", "std", "test", "con", "gpui-kit", "日本",
        ] {
            assert!(crate_name(name).is_err(), "{name:?} was accepted");
        }
    }

    #[test]
    fn defaults_come_from_the_crate_name() {
        assert_eq!(default_display_name("my-app"), "My App");
        assert_eq!(default_display_name("my_cool__app"), "My Cool App");
        assert_eq!(default_app_id("My App", "my-app"), "com.example.MyApp");
        assert_eq!(default_app_id("Notes", "notes"), "com.example.Notes");
        assert_eq!(
            default_app_id("field notes", "notes"),
            "com.example.FieldNotes"
        );
        assert_eq!(default_app_id("Café", "cafe"), "com.example.Cafe");
        assert_eq!(default_app_id("2048", "game"), "com.example.Game");
        assert!(validate_app_id(&default_app_id("My App 2", "my_app2")).is_ok());
    }

    #[test]
    fn app_ids_are_checked() {
        for id in [
            "com.example.MyApp",
            "org.mesa-hills.Notes",
            "io.github.User.App2",
        ] {
            assert_eq!(validate_app_id(id).unwrap(), id);
        }
        for id in [
            "MyApp",
            "com..App",
            "com.example.",
            "com.2example.App",
            "com.my_app.App",
        ] {
            assert!(validate_app_id(id).is_err(), "{id:?} was accepted");
        }
        assert!(validate_app_id(&format!("com.{}", "a".repeat(130))).is_err());
    }

    #[test]
    fn display_names_are_checked() {
        assert_eq!(validate_display_name("  My App ").unwrap(), "My App");
        assert_eq!(
            validate_display_name("Ünïcode Notes").unwrap(),
            "Ünïcode Notes"
        );
        for name in ["", "   ", "Say \"hi\"", "back\\slash", "two\nlines"] {
            assert!(
                validate_display_name(name).is_err(),
                "{name:?} was accepted"
            );
        }
    }

    #[test]
    fn years_of_days() {
        assert_eq!(year_of_day(0), 1970);
        assert_eq!(year_of_day(-1), 1969);
        assert_eq!(year_of_day(11_016), 2000); // 2000-02-29
        assert_eq!(year_of_day(20_088), 2024); // 2024-12-31
        assert_eq!(year_of_day(20_089), 2025); // 2025-01-01
        assert_eq!(year_of_day(20_734), 2026); // 2026-10-08
        assert!(current_year() >= 2026);
    }
}
