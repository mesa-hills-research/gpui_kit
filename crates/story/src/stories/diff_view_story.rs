use gpui_kit::component::{
    ActiveTheme as _,
    diff_view::{Diff, DiffView, DiffViewState},
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, IntoElement, ParentElement as _,
    Render, Styled as _, Window, div, px,
};

use crate::{Story, section};

const OLD: &str = r#"use std::time::Duration;

/// How long a request may take.
const TIMEOUT: Duration = Duration::from_secs(30);

pub fn fetch(url: &str) -> Result<String, Error> {
    let client = Client::new();
    let response = client.get(url).send()?;
    if !response.status().is_success() {
        return Err(Error::Status(response.status()));
    }
    Ok(response.text()?)
}

pub fn fetch_json(url: &str) -> Result<Value, Error> {
    let text = fetch(url)?;
    Ok(serde_json::from_str(&text)?)
}

fn log(message: &str) {
    eprintln!("{message}");
}
"#;

const NEW: &str = r#"use std::time::Duration;

/// How long a request may take.
const TIMEOUT: Duration = Duration::from_secs(10);

pub fn fetch(url: &str) -> Result<String, Error> {
    let client = Client::builder().timeout(TIMEOUT).build()?;
    let response = client.get(url).send()?;
    if !response.status().is_success() {
        return Err(Error::Status(response.status()));
    }
    Ok(response.text()?)
}

pub fn fetch_json(url: &str) -> Result<Value, Error> {
    let text = fetch(url)?;
    Ok(serde_json::from_str(&text)?)
}

fn log(message: &str) {
    tracing::info!("{message}");
}
"#;

const UNIFIED: &str = "diff --git a/README.md b/README.md
--- a/README.md
+++ b/README.md
@@ -1,4 +1,5 @@
 # Fetch
 
-A small HTTP helper.
+A small HTTP helper with a timeout.
+Requests give up after ten seconds.
 
";

pub struct DiffViewStory {
    focus_handle: FocusHandle,
    computed: Entity<DiffViewState>,
    parsed: Entity<DiffViewState>,
}

impl DiffViewStory {
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            computed: cx.new(|cx| DiffViewState::new(Diff::between("src/fetch.rs", OLD, NEW), cx)),
            parsed: cx.new(|cx| DiffViewState::new(Diff::parse(UNIFIED), cx)),
        }
    }

    pub fn view(window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self::new(window, cx))
    }
}

impl Story for DiffViewStory {
    fn title() -> &'static str {
        "Diff View"
    }

    fn description() -> &'static str {
        "Show the changes to files, from two texts or a unified diff."
    }

    fn new_view(window: &mut Window, cx: &mut App) -> Entity<impl Render> {
        Self::view(window, cx)
    }
}

impl Focusable for DiffViewStory {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DiffViewStory {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = |child| {
            div()
                .w(px(640.))
                .h(px(360.))
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius)
                .overflow_hidden()
                .child(child)
        };
        v_flex()
            .gap_6()
            .child(
                section("Between two texts")
                    .description(
                        "Unchanged lines between hunks fold, and the row above a hunk unfolds them.",
                    )
                    .child(frame(DiffView::new(&self.computed))),
            )
            .child(
                section("Unified diff")
                    .description("Parsed from `git diff` output.")
                    .child(frame(DiffView::new(&self.parsed))),
            )
    }
}
