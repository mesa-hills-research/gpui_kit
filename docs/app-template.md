# New apps with mhr_gpui_new

`mhr_gpui_new` writes a GPUI app that looks and behaves like a native app on macOS, Windows and
Linux from its first build: its own title bar with the platform's window controls, the platform's
menus, a window frame on every Linux desktop, an app icon, and release builds that log to a file.

## Usage

```sh
cargo install --git https://github.com/mesa-hills-research/mhr_gpui_kit mhr_gpui_new
mhr_gpui_new <PATH> [--name <NAME>] [--app-id <ID>] [--no-git]
```

| Argument | Meaning |
|---|---|
| `PATH` | Where the app goes, a missing or empty directory. Its last part names the crate, in lower case with other characters turned into hyphens: `My App` becomes `my-app`. |
| `--name` | The name people see in menus, window titles and the About dialog. Defaults to the crate name in title case, `My App`. |
| `--app-id` | A reverse-DNS id such as `com.example.MyApp`, used as the Linux app id and desktop file name, the Windows AppUserModelID and the macOS bundle id. Defaults to `com.example.` and the name without spaces. |
| `--no-git` | Skips `git init`, which otherwise runs when Git is installed and `PATH` is outside any repository. |

## The app

```
Cargo.toml          gpui-kit from this repository, logging, the Windows icon tool
build.rs            embeds the icon and app name in Windows executables
src/main.rs         the app's name and id, logging, startup
src/chrome.rs       window options and can_close, the one close check
src/menus.rs        actions, key bindings and the menus
src/title_bar.rs    the title bar, its menus and buttons
src/workspace.rs    the window's root view
assets/             the icon as SVG and Windows .ico
packaging/linux/    <app id>.desktop and a 256 px icon
README.md
.gitignore
```

| | macOS | Windows | Linux |
|---|---|---|---|
| Title bar | Transparent, with the traffic lights and the window title | Drawn by the app with the menus and caption buttons, which Windows treats as its own (snap layouts on maximize) | Drawn by the app with the menus and the buttons the compositor supports |
| Menus | System menu bar: the app menu with About, Settings, Services, Hide and Quit, then File, Edit and Window | File, Edit and Help in the title bar | As on Windows |
| Frame | System | System shadow, rounded corners and resize borders | The app's own shadow, border and resize edges, on GNOME, KDE, Sway and X11 alike |
| Closing | The close button, ⌘W and ⌘Q all ask `can_close`. Closing the last window keeps the app running, and the Dock icon opens a new window. | The close button, Alt+F4 and Ctrl+Q all ask `can_close` | The close button, the compositor and Ctrl+Q all ask `can_close` |
| Also | | No console window in release builds, the icon and name in Explorer and Task Manager | `app_id` matches the desktop entry |

All three follow the system's light or dark appearance. Debug builds log to the terminal, release
builds to `<APP_ID>/<crate>.log` in the platform's log folder.

Clickable title bar content goes inside `title_bar_item`, which keeps a press on it from starting
a window drag. `can_close` is where an app asks about unsaved work.

## The kit version

The new app's `Cargo.toml` pins `gpui-kit` to the mhr_gpui_kit commit the generator was built
from, so the template and the kit match. A generator built outside a Git checkout of the kit
follows the `main` branch instead.

To build an app against a local checkout of the kit, add a patch to the app's `Cargo.toml` or to
its `.cargo/config.toml`:

```toml
[patch."https://github.com/mesa-hills-research/mhr_gpui_kit"]
gpui-kit = { path = "../mhr_gpui_kit/crates/kit" }
```

## Changing the template

The template lives in `crates/mhr_gpui_new/template` and is compiled into the generator. Text
files take the placeholders `{{crate_name}}`, `{{display_name}}`, `{{app_id}}`, `{{year}}` and
`{{kit_ref}}`. A new file also needs an entry in `FILES` in `src/lib.rs`. Test with

```sh
cargo test -p mhr_gpui_new
```
