# gpui_kit

This repository is Mesa Hills Research's fork of [GPUI Kit](https://github.com/longbridge/gpui-kit)
(formerly gpui-component), the Rust desktop UI framework built on GPUI. It follows upstream
releases and adds the changes listed below.

It is based on GPUI Kit [v0.7.1](https://github.com/longbridge/gpui-kit/tree/v0.7.1), which pins
GPUI to the `gpui-pre` 0.3.8 snapshot, and takes GPUI from the `mhr-gpui` crates on crates.io,
[Mesa Hills Research's fork of Zed's gpui](https://github.com/mesa-hills-research/gpui).

## Changes from upstream

- **Published as `mhr-gpui-kit`**. The published crates are `mhr-gpui-kit`, `mhr-gpui-kit-base`,
  `mhr-gpui-kit-component`, `mhr-gpui-kit-component-macros`, `mhr-gpui-kit-assets` and
  `mhr-gpui-kit-writing`, because `gpui-kit`, `gpui-base` and `gpui-component` on crates.io belong
  to other projects. The libraries keep their names, so `use gpui_kit::*;`, `gpui_base` and
  `gpui_component` work as before. The rest of this page still calls the crates by those names.
- **Managed tooltips on custom elements** (`gpui-component`). Import
  `tooltip::ManagedTooltipExt` and call `.managed_tooltip(...)` on a `div` with an id to use
  Kit's tooltip timing, positioning and open-menu suppression.
- **One focus border on text fields** (`gpui-component`). Inputs, textareas, number inputs,
  OTP cells, input groups and time fields tint their one-pixel border with the theme's `ring`
  color when focused. They draw no outside band, so focus stays visible inside scroll areas.
  Buttons and other controls keep their focus-visible rings.
- **Code blocks on the editor surface** (`gpui-base`, `gpui-component`). Markdown code blocks sit
  on the theme's `editor.background`, where they sat on the muted color, so syntax colors keep the
  contrast the editor gives them in every bundled theme. A 1 px outline in the theme's `border`
  color keeps the edge where that surface matches the page, and `TextViewStyle::with_code_border`
  sets it for other styles. `Theme::editor_background()` is public, so an application that draws
  code of its own can fill the same surface. See
  [Theme: editor colors](website/component/theme.md#editor-colors).
- **Streaming tables grow a row at a time** (`gpui-base`). While text is appended, a table's last
  row that starts with a pipe shows once it is whole. Each word of a row drawn as it came could
  widen its column and re-wrap every row, so a table kept at the bottom of a view jumped up and
  down. Call `TextViewState::finish` when the text is done, so a row it stopped in shows. A
  `TextView::markdown(id, text)` view shows rows as they come. See
  [TextView: retained state and streaming updates](website/base/text-view.md#retained-state-and-streaming-updates).
- **A copy button that confirms** (`gpui-component`). `Clipboard` cross-fades its copy icon to a
  check mark in the theme's success color, holds it for a second and fades back, where the icon
  swapped for two seconds with no motion. Reduced motion swaps them at once. `Clipboard::tab_stop`
  and its styles, which go to the button, let an application use it for copy buttons that show on
  hover. See [Clipboard](website/component/clipboard.md).
- **A selection ends on every release** (`gpui-base`). The window's text selection sees the button
  come up even when a drop target stops the release, as the end of a pane resized by dragging
  does. Text no longer highlights as the pointer moves over it afterwards.
- **Keys still reach a focused row scrolled out of view** (`gpui-base`). While the window has a
  selection, a text view holding focus keeps its keys when a list no longer draws it, so Cmd-C
  or Ctrl-C copies the whole selection, and Select All and the context menu keys still work. See
  [TextView: a long document as rows](website/component/text-view.md#a-long-document-as-rows).
- **Separate views in one document** (`gpui-base`, `gpui-component`). `TextView::document_position`
  and `SelectableText::document_position` place views the application shows apart, such as the
  messages of a conversation, in one document, so a selection across them keeps both ends and
  copies whole while one scrolls out of view. See
  [TextView: a long document as rows](website/component/text-view.md#a-long-document-as-rows).
- **A context menu for selectable text** (`gpui-base`, `gpui-component`). A right-click on a
  selectable `TextView` or `SelectableText`, or Shift-F10 and the Menu key in a focused text view,
  opens a menu with Copy and Select All in the application's context menu style. Copy copies the
  whole window selection as Ctrl-C does, and the right-click leaves the selection as it is.
  `TextSelectionMenu::set_builder` adds an application's own items. See
  [TextView: context menu](website/component/text-view.md#context-menu).
- **A right-click keeps the selection it lands on** (`gpui-base`). A right-click on a selection,
  just past its end or below the text after Select All, opens the menu with the selection kept,
  and so do Shift-F10 and the Menu key, so Copy, Cut and Paste act on it.
- **One context menu style for every text field** (`gpui-component`).
  `ContextMenuStyle::set_app_default` chooses between the operating system's menu and the menu
  GPUI draws in the theme's colors for every input, textarea and editor without a style of its
  own, the ones inside other components too, such as the Settings search field. See
  [Input: context menu](website/component/input.md#context-menu).
- **Long paragraphs with inline code stream faster** (`gpui-base`). A paragraph with inline code,
  images or inline objects lays out and paints only the lines in view, unless a selection needs
  every line, and text streamed onto its end lays out again only the lines it can change.
  Streaming into a 20 KB one went from about 15 to 4 ms a frame, and into an 80 KB one from 65 to
  8 ms (p50).
- **Streamed Markdown parses an append in the same time at any length** (`gpui-base`). The
  parser and the view share the blocks of the parsed document, so an append copies pointers and
  parses only the last block again. In an unoptimized build an append to 162 KB of Markdown takes
  0.6 ms where it took 2.1 ms.
- **A selection across rows survives scrolling** (`gpui-base`). A selection across the rows of
  one document, shown as views in a list, keeps its rows as they scroll out of view. Its ends
  hold, the rows are copied, and they show as selected when they come back. A plain text run
  that holds an end copies only the part that is selected, and a row's position can be any
  `u64`. Shift-click extends the selection from an end scrolled out of view.
  `TextSelectionRegistration::with_document_position` gives other participants the same. See
  [TextView](website/component/text-view.md#a-long-document-as-rows).
- **Less memory after scrolling a long transcript** (`gpui-base`). Text views let go of the
  shaped text of rows a list scrolled past, which they kept for as long as the views lived. One
  pass through 160 answers (318 KB of Markdown) now leaves 2 MB on the heap, where it left 25 MB.
- **Less memory for parsed Markdown** (`gpui-base`). A text view parsed a short text twice, kept
  both documents, and held about 28 KB before it had any text. 160 answers (318 KB of Markdown)
  now take 5.7 MB parsed, where they took 26.8 MB, and parse in half the time. `cargo bench -p
  gpui-base --bench text_view_memory` prints the numbers.
- **The menu bar's keys with nothing focused** (`gpui-component`). F10, a tap of Alt and Alt with
  a menu's letter reach the `AppMenuBar` on Windows and Linux in a window where nothing has focus
  yet, and holding Alt underlines the menus' letters there too. See
  [Title Bar](website/component/title-bar.md#menu-bar-and-the-keyboard).
- **Paging a message scroller** (`gpui-component`). `MessageScrollerState` scrolls by a distance
  with `scroll_by`, reports the viewport's height with `viewport_height`, and keeps and restores
  the reader's place with `logical_scroll_top` and `scroll_to_offset`. See
  [MessageScroller](website/component/message-scroller.md#page-and-keep-the-readers-place).
- **Cmd-Left and Cmd-Right in the Emacs and Vim keys on macOS** (`gpui-base`). They go to the
  start and end of the line, as in CUA and every Mac text field, extending Emacs's active region
  and moving as `0` and `$` in Vim. See
  [Textarea: keybindings](website/component/textarea.md#keybindings).
- **Paragraphs a blank line apart in copied text** (`gpui-base`). Plain-text copy of a text view,
  and `rendered_text()`, put a blank line between blocks, where there was one line break, so
  pasted paragraphs stay apart. See
  [TextView: highlight ranges](website/component/text-view.md#highlight-ranges).
- **The layout of selectable text** (`gpui-base`). `SelectableText::layout` returns the
  `TextLayout` the text is drawn with, so an element around it can find where an offset is drawn
  without shaping the text again. See
  [Text Selection](website/base/text-selection.md#selectable-highlighted-text).
- **A frame rate per shimmer** (`gpui-component`). `ShimmerStyle::max_fps` and
  `ShimmerText::max_fps` set how many frames a second a shimmer draws, 30 by default. See
  [Shimmer](website/component/shimmer.md#styling-theme-and-reduced-motion).
- **Keyboard navigation in tab bars and sidebar menus** (`gpui-base`, `gpui-component`). A
  `TabBar` and a `SidebarMenu` are each one Tab stop, on the selected tab or the active item, and
  the arrow keys, Home and End move focus between their enabled items. Enter or Space activates
  the focused one, and in a sidebar Right and Left open and close submenus. A sidebar taller than
  its window scrolls to the item that takes focus. `RovingFocus` gives other controls the same
  behavior. See [Tabs](website/component/tabs.md#keyboard) and
  [Sidebar](website/component/sidebar.md#keyboard).
- **Cached Settings groups** (`gpui-component`). `SettingGroup::key` reuses unchanged groups
  while editable controls keep their own redraws. Groups without a key keep the existing
  behavior. See [Settings](website/component/settings.md#cache-independent-groups).
- **Settings in narrow windows** (`gpui-component`). Below 480 px, `Settings` shows its search
  field and a page picker above the page instead of the sidebar. See
  [Settings](website/component/settings.md#narrow-widths).
- **Long Settings fields stay in their rows** (`gpui-component`). A field too wide to sit beside
  its title moves below it, and a dropdown label wider than the row is truncated.
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
- **Spell checking and learned word suggestions** (`gpui-kit-writing`, feature `writing` on
  `gpui-kit`). A `Writing` handle loads an English dictionary (SCOWL), the user's own words and
  a vocabulary learned from what they type, and gives every textarea a spell checker that skips
  code, paths and URLs, and a suggestion provider that completes the word and predicts the next,
  paced Off, Quiet or Eager. See [Spell checking and word suggestions](website/docs/writing.md).
- **Send keys for composers** (`gpui-base`). `TextareaState::submit_keys` makes any of Enter,
  Shift+Enter and Cmd/Ctrl+Enter submit a multi-line input without inserting anything, while the
  other Enter chords break lines. `SubmitKey` is a public, serializable setting. The deprecated
  `submit_on_enter(true)` stays, as Enter and Cmd/Ctrl+Enter. See
  [Textarea: send key](website/component/textarea.md#send-key).
- **Keybinding schemes** (`gpui-base`, `gpui-component`). A textarea follows CUA, Emacs or Vim keys
  and switches between them at run time. CUA binds each platform's own text-field shortcuts in
  every input, Emacs adds the mark, the kill ring and prefix arguments, and Vim its modes,
  operators, text objects, registers and command line. See
  [Textarea: keybindings](website/component/textarea.md#keybindings), and
  [docs/keymaps.md](docs/keymaps.md) for working on the schemes.
- **Search-match colors** (`gpui-component`). `search_match` and `search_match_active` theme
  tokens distinguish matches from the current result, with yellow defaults at 25% and 55%
  opacity. See [Theme](website/component/theme.md#search-matches).
- **Inline reference tooltips** (`gpui-component`). `InputToken::tooltip` shows extra context
  on hover, with the same delay and dismissal as other component tooltips. See
  [Input](website/component/input.md#show-a-tooltip-or-preview-on-hover).
- **Selectable highlights** (`gpui-base`). `SelectableText::highlights` styles byte ranges
  while keeping the text selectable and copyable. See
  [Text Selection](website/base/text-selection.md#selectable-highlighted-text).
- **Quiet inactive inputs** (`gpui-base`). Caret blinking stops when the window becomes
  inactive and resumes when its focused input becomes active again.
- **A smooth caret** (`gpui-base`). `smooth_caret(true)` on any input (a single-line input, a
  textarea or the code editor) glides the caret to where it moves. A typed character is
  uncovered by the caret as it passes, and the rest of the line follows it. Off by default. See
  [Textarea: smooth caret](website/component/textarea.md#smooth-caret) and
  [Input: smooth caret](website/component/input.md#smooth-caret).
- **The system's reduced-motion setting, followed** (`gpui-base`). `gpui_base::init` reads it on
  macOS, Windows and Linux, and Base follows changes: macOS's accessibility notification, Linux's
  desktop portal, and on Windows a new reading whenever a window with a Base `Root` becomes
  active. An application that sets the flag itself keeps it. See
  [Motion](website/base/motion.md).
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
- **Code highlighting that keeps up with a stream** (`gpui-base`, `gpui-component`). Fenced code
  blocks are highlighted on a background thread, and a frame spends about 20 µs on a block's
  highlights, where a chunk of a 200 KB block of flat code such as Python, JSON or Markdown took 12
  to 32 ms on the UI thread. Each block keeps its parse and highlights, a block that streams in
  is parsed and highlighted again from where it changed, and it's highlighted once more from
  scratch when its fence closes. A block shown in part is highlighted as far as its shown lines.
  Highlights are shared with the block rather than copied every frame, and a theme change
  recolors them without parsing. See
  [TextView: code highlighting while streaming](website/component/text-view.md#code-highlighting-while-streaming).
- **More code languages** (`gpui-component`). Dart, Elm, Erlang, F#, GLSL, Haskell, HCL, Julia,
  Objective-C, OCaml, Perl, PowerShell, R and XML highlight with Mesquite's grammars and their
  projects' queries, and fences find their grammar by the names they usually carry (`shell`,
  `zsh`, `console`, `jsx`, `patch`, `ps1`, `tf`, `golang` and more).
- **Languages prepared off the UI thread** (`gpui-component`). A grammar is decoded only when
  something highlights its language, where enabling `tree-sitter-languages` decoded all 35 at the
  first highlight, which took about a second and 75 MB. A code block whose language is not ready
  shows plain code while the language compiles on a background thread, and
  `LanguageRegistry::warm_up` prepares languages ahead of time. See
  [TextView: code highlighting while streaming](website/component/text-view.md#code-highlighting-while-streaming).
- **Long answers as list rows** (`gpui-base`). `TextViewState::block_range` shows some top-level
  blocks of a document that another `TextViewState` parses, and `TextViewState::blocks` lists them
  with stable ids, so a chat transcript can show a long answer as rows of a list and lay out only
  the rows on screen, 0.9 ms a frame instead of 12.5 ms while a 20,000-token answer streams.
  Selection runs across the rows. `TextViewState::markdown_extensions` gives the document its
  Markdown extensions and plugins, which its rows render with, and a `TextView` given none leaves
  its state's as they are. `TextView::code_block_max_lines` shows the first lines of each code
  block. See
  [TextView: a long document as rows](website/component/text-view.md#a-long-document-as-rows).
- **Loading indicators at 30 frames a second** (`gpui-component`). Skeleton, the loading Progress
  and ProgressCircle, ShimmerText and the shimmer of loading Markers draw at most
  `LOADING_ANIMATION_MAX_FPS` (30) frames a second, so an indicator left up through a long request
  does not keep the window drawing at the display's full rate. Several indicators draw in the same
  frames, so a window showing three draws 30 frames a second, not 90.
- **A spinner that turns evenly** (`gpui-component`). `Spinner` turns at a constant speed in each
  frame the display shows, where it used to ease in and out once in each turn, which made it stall
  and then jump. `Spinner::max_fps` caps it and `Spinner::duration` sets the time of a turn. It
  turns as it paints, so the view around it is not drawn again for each step.
- **Steady memory while a block streams** (`gpui-base`). The shaped text a block keeps for the
  next frame is released when the block is replaced, where every chunk of a long streamed block
  used to leave a copy of its layout behind.
- **Ghost text that makes room** (`gpui-base`). An inline completion in the middle of a line moves
  the rest of the line right, and the line wraps with it, where upstream painted over the text.
- **Meters with a limit and segments** (`gpui-component`). `ProgressCircle` and `Progress` mark a
  value with a tick, stack labelled `ProgressSegment`s along the ring or bar, take a track colour
  (and the ring a stroke width), and read out a value text such as "142k of 1M tokens" instead of
  a bare percentage. `UsageBar` shows a bar with its label, its amount and a legend of its
  segments in text. See [Progress](website/component/progress.md#usagebar).
- **Work Sans and JetBrains Mono** (`gpui-kit-assets`). The `fonts` feature embeds both as
  variable fonts, upright and italic, so medium, semibold and bold text use the real weights.
  `gpui_kit::assets::fonts::all()` goes straight to `add_fonts`. See
  [Fonts: bundled Work Sans and JetBrains Mono](website/docs/fonts.md#bundled-work-sans-and-jetbrains-mono).
- **Bundled themes, a user folder, and fonts that stay** (`gpui-component`). `ThemeRegistry`
  keeps themes an app embeds (`add_bundled_themes_from_str`) across reloads of the watched
  folder, whose themes replace bundled ones of the same name ignoring case. The folder is read
  on a background thread (`ThemeRegistry::load_dir` reads a folder once, without a watcher, for
  tests). `Theme::set_font_overrides` sets the app's UI and mono fonts and sizes once, and every
  theme applied after keeps them, in one update. See
  [Theme: font overrides](website/component/theme.md#font-overrides).
- **Code with or without ligatures** (`gpui-base`, `gpui-component`).
  `FontOverrides::with_mono_font_features` sets the monospace font's OpenType features, such as
  `calt` and `liga` turned off, and every theme applied after keeps them. Code blocks and inline
  code in text views, the diff view, the code editor and key caps use them, and text in the UI
  font keeps its own. See
  [Theme: code font features](website/component/theme.md#code-font-features).
- **Sidebars and sheets that can snap** (`gpui-component`). `Sidebar::transition_duration` and
  `Sheet::transition_duration` set how long collapsing or sliding in takes, `Duration::ZERO`
  snaps, and both default to the theme's normal motion duration instead of fixed 200 ms and
  150 ms. See [Sidebar: collapse transition](website/component/sidebar.md#collapse-transition).
- **An image cache with a budget** (`gpui-base`). `LruImageCache` unloads the least recently
  drawn images once their decoded size passes a budget, never one still on screen, and decodes
  in-memory images (`LruImageCache::source`) on a background thread, scaled to fit a thumbnail
  if asked. It refuses an image larger than the whole budget from its header, before decoding.
  GPUI otherwise keeps every `ImageSource::Image` for the life of the app. See
  [Image Cache](website/base/image-cache.md).
- **An image viewer** (`gpui-component`). `window.open_image_viewer(items, ix, cx)` shows a set of
  images over the window, fitted or at actual size (Z or a double-click), with a "2 / 5"
  counter and captions. ←/→, Page Up/Page Down and Home/End browse, the arrows pan an image
  at actual size, and Escape closes it and gives focus back to the thumbnail. Dialogs stack
  with it in the order they open. It only fades in, and not at all under reduced motion. See
  [Image Viewer](website/component/image-viewer.md).
- **Background font catalogs** (`gpui-component`). `FontCatalog::load` enumerates installed
  families on the background executor. Pickers keep their search and selection as fonts arrive,
  and `FontPickerState::set_catalog` updates an open picker in place. See
  [FontPicker](website/component/font-picker.md#loading-and-replacing-a-catalog).
- **Font pickers with only the rows a setting keeps** (`gpui-component`).
  `FontPicker::show_weight`, `show_line_height` and `show_features` leave those rows out, for a
  setting that stores only a family and a size. `show_monospace_filter(false)` hides the
  monospace checkbox without changing the filter. See
  [FontPicker](website/component/font-picker.md#compact-layout).
- **A diff view** (`gpui-component`). `DiffView` shows `Diff::between(path, old, new)` or
  `Diff::parse(unified)` with old and new line numbers, added and removed lines on the
  success and danger colors and marked `+`/`-`, hunk headers, syntax colors by extension
  (found off the UI thread, colored at paint), unchanged lines that fold and unfold, line
  break and final-newline changes, renames, modes and binary files, and a Copy button per
  file, in a virtualized list that can size to its rows and scrolls by keyboard. See
  [Diff View](website/component/diff-view.md).
- **Window chrome per platform** (`gpui-component`). Windows gets Windows 11 caption buttons (46
  px, Segoe Fluent Icons glyphs, the close red, disabled for windows that can't be resized or
  minimized, dimmed while inactive), Linux buttons follow the desktop's button layout, sides and
  order, and the macOS inset drops in full screen. F10 or an Alt tap focuses the `AppMenuBar`,
  arrows move, Alt with a menu's underlined letter opens it. See
  [TitleBar: menu bar and the keyboard](website/component/title-bar.md#menu-bar-and-the-keyboard).
  `TitleBar::center` centers a title on the window, moving it aside for the content beside it
  and narrowing it when the bar runs out of room, and `suffix` puts content just before the
  window buttons. See [Centered title](website/component/title-bar.md#centered-title) on the
  TitleBar page.
- **GPUI from the gpui fork.** The kit pins the published `mhr-gpui` crates, each to an exact
  version, where upstream pins the `gpui-pre` snapshots. To work on both repositories together,
  see [crates/kit/TESTING.md](crates/kit/TESTING.md#working-on-the-gpui-fork-alongside).
- **Golden screenshots** (`crates/kit/tests/screenshots.rs`). Buttons, inputs, a textarea and its
  suggestion menu, a text editor, its spelling menu and line-number gutters, a list and a popup
  menu render headlessly on Linux, in light and dark themes and at scale factors 2 and 2.625,
  and are compared with the PNGs in `crates/kit/tests/screenshots`. They run with the kit's
  tests and use the gpui fork's
  `mhr-gpui-screenshot`. [crates/kit/TESTING.md](crates/kit/TESTING.md#golden-screenshots-on-linux)
  covers running them and updating the goldens.
- **Tooltips that leave with their trigger** (`gpui-base`, `gpui-component`). A component's tooltip
  is drawn only on frames that draw the element it describes, so it disappears with a button
  whose click closes its page or a row that re-renders away, and one waiting for its show delay
  never appears. `TooltipTrigger` and `TooltipRequest::for_trigger` give a custom trigger the
  same. See [Tooltip: when the trigger goes away](website/component/tooltip.md#when-the-trigger-goes-away).
- **Tooltips along a row of buttons** (`gpui-base`, `gpui-component`, `gpui-shell`). While a
  tooltip is up, the next button's replaces it once the pointer rests there for a tenth of a
  second, so sweeping across a row of buttons leaves the open tooltip in place instead of flashing
  each button's. The box then moves and resizes to the new tooltip and uncovers its text, which
  stays where it is. See
  [Tooltip: moving between tooltips](website/component/tooltip.md#moving-between-tooltips).
- **No tooltip over an open menu** (`gpui-base`, `gpui-component`). While a dropdown menu,
  popover or context menu is open, its trigger and the elements inside it show no tooltip, so none
  covers the menu when the pointer comes back to the trigger. `prepaint_popup_trigger` gives a
  custom popup host the same. See
  [Tooltip: while a menu is open](website/component/tooltip.md#while-a-menu-is-open).
- **A generator for new apps** (`gpui_new`), described in [Starting a new app](#starting-a-new-app).

## Using it

```toml
[dependencies]
mhr-gpui-kit = "0.7"
```

The kit depends on the published `mhr-gpui` crates, which Cargo fetches from crates.io, so the app
needs no `[patch]` and gets the fork's fixes, such as variable fonts at the requested weight on
Linux. The app's code still writes `use gpui_kit::*;`. The fork's
[docs/using.md](https://github.com/mesa-hills-research/gpui/blob/main/docs/using.md) covers
using the crates without the kit, and apps from `gpui_new` start from this setup.

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
mhr-gpui-kit                 The one crate applications depend on
├── mhr-gpui-kit-base        Unstyled behavior, state, and infrastructure
└── mhr-gpui-kit-component   GPUI Component: the complete styled UI system
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
mhr-gpui-kit = "0.7"
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

- Built on [GPUI](https://github.com/zed-industries/zed), the UI framework from Zed Industries, also Apache-2.0. The `mhr-gpui` crates are a fork of it, published with Zed's license and notices intact.
- UI design based on [shadcn/ui](https://ui.shadcn.com), some from [Reui](https://reui.io).
- Icons from [Lucide](https://lucide.dev).
