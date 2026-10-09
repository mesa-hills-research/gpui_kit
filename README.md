# gpui_kit

This repository is Mesa Hills Research's fork of [GPUI Kit](https://github.com/longbridge/gpui-kit)
(formerly gpui-component), the Rust desktop UI framework built on GPUI. It follows upstream
releases and adds the changes listed below.

It is based on GPUI Kit [v0.7.1](https://github.com/longbridge/gpui-kit/tree/v0.7.1), which pins
GPUI to the `gpui-pre` 0.3.8 snapshot, and takes GPUI from
[Mesa Hills Research's fork of those crates](https://github.com/mesa-hills-research/gpui).

## Changes from upstream

- **Steady text when typing at the end of a Textarea** (`gpui-base`). Multi-line inputs clamp the
  vertical scroll offset before laying out text, so the frame of each keystroke paints the text
  where the next frame does.
- **Suggestions in a Textarea** (`gpui-base`, `gpui-component`). A `TextareaState` takes a
  `SuggestionProvider` and offers its suggestions in a menu under the word, as ghost text after the
  caret, or both, while staying a plain text area. Answers for text that has since changed are
  dropped, and the provider hears every edit with a revision. See
  [Textarea: suggestions](website/component/textarea.md#suggestions).
- **A text editor** (`gpui-base`, `gpui-component`). `TextareaState::text_editor` and
  `TextEditor` make a textarea a prose editor with line numbers, search and the application's
  spell checker. Misspelled words get wavy underlines that move with edits, and a right-click on
  one, or Shift-F10 and the Menu key at the caret, offers the checker's suggestions, Add to
  Dictionary and Ignore. Line numbers, marks (underlines that follow the text) and a context menu
  that knows the clicked word work on any textarea. See
  [Textarea: text editor](website/component/textarea.md#text-editor).
- **Keybinding schemes** (`gpui-base`, `gpui-component`). A textarea follows CUA, Emacs or Vim keys
  and switches between them at run time. CUA binds each platform's own text-field shortcuts in
  every input, Emacs adds the mark, the kill ring and prefix arguments, and Vim its modes,
  operators, text objects, registers and command line. See
  [Textarea: keybindings](website/component/textarea.md#keybindings), and
  [docs/keymaps.md](docs/keymaps.md) for working on the schemes.
- **A smooth caret** (`gpui-base`). `smooth_caret(true)` on a textarea or the code editor glides
  the caret to where it moves. A typed character is uncovered by the caret as it passes, and the
  rest of the line follows it. Off by default. See
  [Textarea: smooth caret](website/component/textarea.md#smooth-caret).
- **Nested scrolling** (`gpui-component`). A List or an `overflow_*_scrollbar` area inside a
  scrolling page keeps the wheel while it can scroll, where GPUI scrolled the page along with
  it. At its end the page takes over with the next gesture. See
  [Scrollable: inside another scroll area](website/component/scrollable.md#inside-another-scroll-area).
- **Compact line numbers** (`gpui-base`). The line-number gutter of the code editor and of a
  textarea is as wide as the last line's number needs, from two digits, with the numbers
  right-aligned, and leaves room for folding markers only while folding is on.
- **Status colours without tree-sitter** (`gpui-component`). Without the `tree-sitter` feature,
  diagnostic and status colours fall back to the theme's red, yellow, blue, green and cyan, as
  they do with it, where upstream made them transparent.
- **Highlighting through Mesquite** (`gpui-component`). The `tree-sitter` features take
  tree-sitter and its grammars from [Mesquite](https://github.com/mesa-hills-research/mesquite),
  a pure Rust port, so they build without a C compiler and cross-compile to Windows without
  MinGW headers. Kotlin and Svelte use the tree-sitter-kotlin-ng and tree-sitter-svelte-ng
  grammars. See [Editor: basic usage](website/component/editor.md#basic-usage).
- **Ghost text that makes room** (`gpui-base`). An inline completion in the middle of a line moves
  the rest of the line right, and the line wraps with it, where upstream painted over the text.
- **GPUI from the gpui fork.** A `[patch.crates-io]` in `Cargo.toml` points the 25 `gpui-pre`
  crates at one commit of the fork on GitHub. To work on both repositories together, see
  [crates/kit/TESTING.md](crates/kit/TESTING.md#working-on-the-gpui-fork-alongside).
- **Golden screenshots** (`crates/kit/tests/screenshots.rs`). Buttons, inputs, a textarea and its
  suggestion menu, a text editor, its spelling menu and line-number gutters, a list and a popup
  menu render headlessly on Linux, in light and dark themes and at scale factors 2 and 2.625,
  and are compared with the PNGs in `crates/kit/tests/screenshots`. They run with the kit's
  tests and use the gpui fork's
  `gpui-pre-screenshot`. [crates/kit/TESTING.md](crates/kit/TESTING.md#golden-screenshots-on-linux)
  covers running them and updating the goldens.
- **A generator for new apps** (`gpui_new`), described in [Starting a new app](#starting-a-new-app).

## Using it

The crates keep upstream's names, so an app moves to the fork by changing where `gpui-kit` comes
from:

```toml
[dependencies]
gpui-kit = { git = "https://github.com/mesa-hills-research/gpui_kit", rev = "<commit>" }
```

Cargo applies only the app's own patches, so the app also points the `gpui-pre` crates at the
gpui fork, at the commit this repository's `Cargo.toml` names. Otherwise it builds with their
crates.io release, without the fork's fixes, such as variable fonts at the requested weight on
Linux:

```toml
[patch.crates-io]
gpui-pre = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
# and each other gpui-pre crate in the app's Cargo.lock
```

The fork's [docs/using.md](https://github.com/mesa-hills-research/gpui/blob/main/docs/using.md)
lists every entry, and apps from `gpui_new` come with them.

## Starting a new app

`gpui_new` creates an app with the title bar, menus, window frame, icon and logging set up for
macOS, Windows and Linux:

```sh
cargo install --git https://github.com/mesa-hills-research/gpui_kit gpui_new
gpui_new my-app --name "My App" --app-id com.example.MyApp
cd my-app && cargo run
```

[docs/app-template.md](docs/app-template.md) describes the generated app.

## Following upstream

Each upstream release lands as one commit that holds the release as tagged, minus upstream's
Dependabot config. The fork's own changes follow as separate commits, each with its test.

---

*Upstream's README follows.*

<p align="center">
  <img src="https://raw.githubusercontent.com/longbridge/gpui-kit/main/website/public/logo.svg" width="112" alt="GPUI Kit logo" />
  <br>
  <strong>GPUI Kit</strong>
</p>

[English](./README.md) | [简体中文](./README.zh-CN.md)

[![Build Status](https://github.com/longbridge/gpui-kit/actions/workflows/ci.yml/badge.svg)](https://github.com/longbridge/gpui-kit/actions/workflows/ci.yml) [![Docs](https://docs.rs/gpui-kit/badge.svg)](https://docs.rs/gpui-kit/) [![Crates.io](https://img.shields.io/crates/v/gpui-kit.svg)](https://crates.io/crates/gpui-kit)

Build fantastic, high-performance desktop apps with Rust and GPUI.

GPUI Kit is a comprehensive Rust desktop application framework. It combines a
production-ready UI system with application-grade data, layout, and editing
capabilities, all built on a reusable foundation of behavior, state, and
infrastructure. GPUI Kit ships 75+ documented components and primitives,
WebAssembly support, AccessKit accessibility, UI integration testing, and an
optional JavaScript extension runtime.

Documentation: <https://gpui-kit.com>

```text
gpui-kit             The one crate applications depend on
├── gpui-base        Unstyled behavior, state, and infrastructure
└── gpui-component   GPUI Component: the complete styled UI system
```

`gpui-kit` pins the matching GPUI release and re-exports GPUI, base, component,
and assets, so a Rust application lists a single dependency. JavaScript extension
hosts add `gpui-shell` separately; `gpui-component-shell` supplies the styled catalog.

See the [executable application recipe and AI-assisted development acceptance checks](examples/ai_recipes/README.md) for a tested starting point and verification commands.

## Features

- **75+ Components and Primitives**: Forms, navigation, overlays, data display, editing, feedback, and layout, with polished interactions and productive defaults.
- **Production Ready**: Used to build Longbridge Pro from day one and continuously refined in a publicly shipped commercial desktop application.
- **WebAssembly**: Run applications and the same component showcases on the web with `wasm32-unknown-unknown`.
- **Accessibility**: AccessKit roles, names, states, relationships, and actions are built into the interaction layer and covered by tests.
- **UI Integration Testing**: Render real components in headless windows, drive pointer and keyboard input, and assert state, focus, layout, and accessibility.
- **Native Feel**: Modern controls inspired by macOS and Windows, backed by semantic themes and multiple sizes.
- **120 FPS**: GPU-accelerated interfaces that remain smooth under load.
- **Data Tables**: Virtual scrolling, fixed and resizable columns, sorting, and cell selection across hundreds of thousands of rows.
- **Virtual Lists**: Render only the visible range, including lists whose items have different sizes.
- **Code Editor**: Stable performance at 200K lines with Tree-sitter highlighting and LSP diagnostics, completion, and hover.
- **Dock Layout**: Resizable panels, draggable tabs, nested splits, and edge docks — all serializable.
- **Rich Content**: Native Markdown and HTML rendering, syntax highlighting, and built-in charts.
- **Design Freedom**: Use the complete visual system or build your own on the behavior and infrastructure in `gpui-base`.
- **JavaScript Extensions**: `gpui-shell` lets a shipped Rust host load panels and business logic as scripts, with every capability granted explicitly.
- **Cross Platform**: Ship one Rust codebase to macOS, Windows, and Linux.

## Framework Architecture

### Three layers. One ecosystem.

Use `gpui-component` to keep the application coherent with one complete visual
and interaction system. Use `gpui-base` when your product needs to create and
own that system itself. Use `gpui-shell` when the application should be
extensible in JavaScript after it ships.

| **`gpui-component`**             | **`gpui-base`**                               | **`gpui-shell`**                           |
| -------------------------------- | --------------------------------------------- | ------------------------------------------ |
| Complete, styled components      | Unstyled behavior and infrastructure          | JavaScript runtime hosted by Rust          |
| Productive defaults with theming | Full control over structure and visual design | Capabilities granted one at a time         |
| Best for building applications   | Best for building design systems              | Best for plugins and scripted applications |

```text
                             APPLICATION
                                  │
              ┌───────────────────┼───────────────────┐
              │                   │                   │
              ▼                   ▼                   ▼
    ┌──────────────────┐ ┌──────────────────┐ ┌──────────────────┐
    │  gpui-component  │ │ Your Design      │ │    gpui-shell    │
    │    Styled UI     │ │ System           │ │  JS extensions   │
    └────────┬─────────┘ └────────┬─────────┘ └────────┬─────────┘
             │                    │                    │
             └────────────────────┼────────────────────┘
                                  ▼
                        ┌──────────────────┐
                        │    gpui-base     │
                        │ Behavior · State │
                        │ Infrastructure   │
                        └────────┬─────────┘
                                 ▼
                               GPUI
```

> **Behavior belongs to the foundation. Presentation belongs to the application.**

Use **`gpui-component`** when you want polished controls ready to ship. Build on
**`gpui-base`** when your application should own its component source, layout,
styling, and motion while reusing difficult interaction behavior. Add
**`gpui-shell`** when contributors should extend the product without a fork or
a release.

The layering follows the same separation that makes the
[shadcn](https://ui.shadcn.com) ecosystem flexible:

| GPUI Kit ecosystem                   | Web ecosystem                   |
| ------------------------------------ | ------------------------------- |
| GPUI                                 | HTML + Tailwind CSS             |
| [`gpui-base`](crates/base/README.md) | [Base UI](https://base-ui.com)  |
| `gpui-component`                     | shadcn's styled component layer |

[Explore the architecture →](docs/ARCHITECTURE.md)

## Showcase

GPUI Kit has powered [Longbridge Pro](https://longbridge.com/desktop)
from day one. The framework is extracted from the demands of a publicly shipped
commercial desktop application rather than designed in isolation.

> **GPUI provides the rendering foundation. Longbridge provides the production foundation.**

<img width="1763" alt="Image" src="https://github.com/user-attachments/assets/e1ecb9c3-2dd3-431e-bd97-5a819c30e551" />

## Usage

```toml
[dependencies]
gpui-kit = "0.7"
```

`gpui-kit` always brings in GPUI and `gpui-base`; `gpui-component` and the
default icon set are on by default. Turn default
features off to keep only the layers you use. The `gpui-component` features (`inspector`, `decimal`,
`tree-sitter`, and each `tree-sitter-<language>`) are available under the same
names.

### Basic Example

```rs
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;

pub struct HelloWorld;
impl Render for HelloWorld {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .gap_2()
            .size_full()
            .items_center()
            .justify_center()
            .child("Hello, World!")
            .child(
                Button::new("ok")
                    .primary()
                    .label("Let's Go!")
                    .on_click(|_, _, _| println!("Clicked!")),
            )
    }
}

fn main() {
    gpui_kit::application().run(move |cx| {
        // This must be called before using any GPUI Component features.
        gpui_kit::init(cx);

        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(|_| HelloWorld)
        })
        .expect("Failed to open window");
    });
}
```

`gpui_kit::open_window` is the application window entry point and always mounts a Base `Root`. Component initialization registers styled window facilities; Cargo features do not select a different root type.

### Icons

The default `assets` feature bundles the [Lucide](https://lucide.dev) icon set
as `gpui-kit-assets`; pass it to the application with
`gpui_kit::application().with_assets(gpui_kit::assets::Assets)`. To ship your
own icons instead, leave that feature out and name the SVG files as defined in
[IconName](https://github.com/longbridge/gpui-kit/blob/main/crates/component/src/icon.rs#L86).

## Skills for AI Coding Agents

Install the GPUI Kit skills for your AI coding agent (Cursor, Claude Code, Gemini CLI, Codex, etc.):

```bash
npx skills add longbridge/gpui-kit
```

| Skill                    | Description                                                                                                                         |
| ------------------------ | ----------------------------------------------------------------------------------------------------------------------------------- |
| `gpui-kit`               | Setup, component catalog, usage patterns, GPUI mechanics (elements, entities, async, focus, actions, tests), and the Coding Guides. |
| `gpui-kit-design-guides` | The Design Guides: layout, spacing, hierarchy, interaction states, overlays, and interface copy.                                    |

## Development

### Desktop Gallery (Story)

The `story` crate is a gallery application that showcases all available components. Run it with:

```bash
cargo run
```

### Examples

Some larger examples reuse the `story` gallery components and run as standalone packages:

```bash
# Dock layout system (panels, split views, tabs)
cargo run -p example-dock

# Markdown rendering
cargo run -p example-markdown

# HTML rendering
cargo run -p example-html
```

The `examples` directory also contains standalone examples, each focused on a single feature. Each example is a separate crate, run them with `cargo run -p <name>`:

```bash
# Code editor with LSP support and syntax highlighting
cargo run -p example-editor

# Basic hello world
cargo run -p hello_world

# System monitor (real-time charts with CPU/memory data)
cargo run -p system_monitor

# Window title customization
cargo run -p window_title
```

Check out [CONTRIBUTING.md](CONTRIBUTING.md) for more details.

## Compare to others

See the [comparison with Iced, egui and Qt 6](https://gpui-kit.com/docs/comparison) on the site.

## License

Software source and documentation code examples: [Apache-2.0](LICENSE-APACHE).

Documentation prose and original illustrations in the Docs, Base, Component, and Shell sections (including Chinese translations) for which GPUI Kit holds licensing rights are also offered under [CC BY 4.0](LICENSE-DOCS.md). When copying or adapting that material, credit **GPUI Kit**, link to the source page and [CC BY 4.0 license](https://creativecommons.org/licenses/by/4.0/), and indicate changes. Existing Apache-2.0 permissions remain; earlier revisions retain their prior terms, and third-party contributions keep their own licenses unless separately authorized. Using facts or ideas without copying protected expression does not require attribution under CC BY 4.0.

- Built on [GPUI](https://github.com/zed-industries/zed), the UI framework from Zed Industries, also Apache-2.0. The `gpui-pre-*` crates are snapshots of it, published with Zed's license and notices intact.
- UI design based on [shadcn/ui](https://ui.shadcn.com), some from [Reui](https://reui.io).
- Icons from [Lucide](https://lucide.dev).
