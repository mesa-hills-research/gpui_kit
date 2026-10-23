---
title: Tabs
description: A tab list and accessible tab controls with controlled selection.
order: 30
---

# Tabs

A tab list and accessible tab controls with controlled selection.

Like every `gpui-base` primitive, Tabs supplies behavior and semantic structure without imposing a product visual language. Apply GPUI styles and compose the exported parts to match your design system.

## Example

The [single native Cargo entrypoint](https://github.com/longbridge/gpui-kit/blob/main/crates/base/examples/native/src/bin/components.rs) selects this primitive from the [shared showcase implementation](https://github.com/longbridge/gpui-kit/blob/main/crates/base/examples/showcase/mod.rs). The same showcase is compiled once for the WASM preview above.

```bash
cargo run -p gpui-base-examples -- tabs
```

## Import

```rust
use gpui_kit::base::{Tab, Tabs};
```

## Anatomy and API

The example composes `Tab`, `Tabs`. GPUI's standard styling and event traits provide presentation; these base types provide the interaction structure.

The authoritative module is [`components/tabs.rs`](https://github.com/longbridge/gpui-kit/blob/main/crates/base/examples/showcase/components/tabs.rs). Native and browser previews compile this same file.

## State and events

The parent owns selected index/value; each tab reflects it and click handlers update the parent.

Keep controlled state on the parent render type or in a GPUI entity. Update it in callbacks and call `cx.notify()`; do not recreate persistent entities during every render.

## Complete Rust example

The complete implementation used by the runnable showcase is embedded directly from Rust source:

<<< ../../../crates/base/examples/showcase/components/tabs.rs{rust}

The command above supplies application initialization, window creation, and shared `BaseShowcase` state.

## Keyboard

`Tabs::roving_focus` gives a tab list the keyboard behavior of a desktop tab control. A `RovingFocus` keeps one focus handle per tab, and the list is one Tab stop, on the current tab. Left and Right move focus to the previous and next enabled tab, wrapping at the ends, and Home and End move to the first and last. Enter or Space on the focused tab runs its `on_click`.

```rust
use gpui_kit::base::{RovingFocus, Tab, Tabs};

impl Render for Preferences {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let labels = ["General", "Appearance", "Advanced"];
        let focus = RovingFocus::keyed("preferences-tabs-focus", window, cx);
        focus.set_items(labels.iter().map(|_| true), Some(self.selected), cx);

        Tabs::new("preferences-tabs")
            .roving_focus(&focus)
            .flex()
            .children(labels.into_iter().enumerate().map(|(ix, label)| {
                Tab::new(ix)
                    .selected(ix == self.selected)
                    .track_focus(&focus.handle(ix).unwrap())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = ix;
                        cx.notify();
                    }))
                    .child(label)
            }))
    }
}
```

Other composite controls whose items share one Tab stop, such as a vertical list, can use `RovingFocus` directly and call `move_focus` from their key handlers.

A control in a scrolling container can scroll the item that takes focus into view. `track_scroll` takes the `ScrollHandle` of a container with one child per item. `track_list_scroll` takes the `ListState` of a GPUI `list` whose items may each hold many of the control's items, as in a sidebar. `track_container_scroll` takes the `ScrollHandle` of a container that holds the control at any depth, among other content. With either one, each item reports where it is drawn with `item_prepainted`. The list or container scrolls by the least distance that shows an item that took focus from the keyboard, and it doesn't scroll when the item is in view already.

## Accessibility

Associate tabs with panels, expose selection, and support keyboard traversal.

## Notes

Use stable element IDs where accepted. Verify focus, hover, active, selected, disabled, reduced-motion, and high-contrast appearances in the consuming design system.
