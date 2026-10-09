use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use mhr_gpui_new::{Error, Project, generate};

const USAGE: &str = "\
Creates a GPUI app with the title bar, menus and window frame set up for macOS,
Windows and Linux.

Usage: mhr_gpui_new <PATH> [OPTIONS]

The crate is named after the last part of PATH, which must be missing or empty.

Options:
      --name <NAME>      Name people see in menus and window titles [default: from PATH]
      --app-id <ID>      Reverse-DNS app id, such as com.example.MyApp [default: com.example.<Name>]
      --no-git           Skip `git init`
  -h, --help             Print this help
  -V, --version          Print the version";

struct Args {
    path: PathBuf,
    name: Option<String>,
    app_id: Option<String>,
    git: bool,
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(Some(args)) => args,
        Ok(None) => return ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<(), Error> {
    let project = Project::new(&args.path, args.name.as_deref(), args.app_id.as_deref())?;
    generate(&args.path, &project)?;

    let git = args.git && git_init(&args.path);
    println!(
        "Created {} in {} (crate {}, app id {}{})",
        project.display_name,
        args.path.display(),
        project.crate_name,
        project.app_id,
        if git { ", new Git repository" } else { "" },
    );
    if args.app_id.is_none() {
        println!("Pass --app-id to replace the example app id with your own.");
    }
    println!("\n    cd {}\n    cargo run", args.path.display());
    Ok(())
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Option<Args>, Error> {
    let mut path = None;
    let mut name = None;
    let mut app_id = None;
    let mut git = true;
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag.to_string(), Some(value)),
            _ => (arg.clone(), None),
        };
        let mut value = |flag: &str| match inline {
            Some(value) => Ok(value.to_string()),
            None => args
                .next()
                .ok_or_else(|| Error::new(format!("{flag} needs a value"))),
        };
        match flag.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("mhr_gpui_new {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--name" => name = Some(value("--name")?),
            "--app-id" => app_id = Some(value("--app-id")?),
            "--no-git" => git = false,
            _ if arg.starts_with('-') && arg != "-" => {
                return Err(Error::new(format!("unknown option {arg}")));
            }
            _ if path.is_none() => path = Some(PathBuf::from(arg)),
            _ => return Err(Error::new(format!("unexpected argument {arg}"))),
        }
    }
    let path = path.ok_or_else(|| Error::new("missing the PATH of the new app"))?;
    Ok(Some(Args {
        path,
        name,
        app_id,
        git,
    }))
}

/// Starts a Git repository in `dir`, unless Git is missing or `dir` is
/// already inside a repository.
fn git_init(dir: &Path) -> bool {
    let git = |args: &[&str]| Command::new("git").arg("-C").arg(dir).args(args).output();
    match git(&["rev-parse", "--is-inside-work-tree"]) {
        Err(_) => return false,
        Ok(output) if output.status.success() => return false,
        Ok(_) => {}
    }
    git(&["init", "--quiet"]).is_ok_and(|output| output.status.success())
}
