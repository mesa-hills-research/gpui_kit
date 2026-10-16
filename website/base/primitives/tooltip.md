---
title: Tooltip
description: A delayed, positioned description associated with a trigger element.
order: 34
---

# Tooltip

A delayed, positioned description associated with a trigger element.

Like every `gpui-base` primitive, Tooltip supplies behavior and semantic structure without imposing a product visual language. Apply GPUI styles and compose the exported parts to match your design system.

## Example

The [single native Cargo entrypoint](https://github.com/longbridge/gpui-kit/blob/main/crates/base/examples/native/src/bin/components.rs) selects this primitive from the [shared showcase implementation](https://github.com/longbridge/gpui-kit/blob/main/crates/base/examples/showcase/mod.rs). The same showcase is compiled once for the WASM preview above.

```bash
cargo run -p gpui-base-examples -- tooltip
```

## Import

```rust
use gpui_kit::base::{Tooltip};
```

## Anatomy and API

The example composes `Tooltip`. GPUI's standard styling and event traits provide presentation; these base types provide the interaction structure.

The authoritative module is [`components/tooltip.rs`](https://github.com/longbridge/gpui-kit/blob/main/crates/base/examples/showcase/components/tooltip.rs). Native and browser previews compile this same file.

## State and events

Hover or focus schedules it and exit or blur dismisses it; content is descriptive.

Keep controlled state on the parent render type or in a GPUI entity. Update it in callbacks and call `cx.notify()`; do not recreate persistent entities during every render.

### Tying a tooltip to its trigger

`TooltipOverlay` shows a `TooltipRequest`. A request made with
`TooltipRequest::for_trigger` is drawn only on frames that also draw its
trigger, so the tooltip disappears with the element it describes, such as a
button whose click closes its page, and one still waiting for its show delay
never appears. Create a `TooltipTrigger` when building the trigger element and
record its bounds from the element's prepaint on every frame:

```rust
use gpui_kit::base::{ElementExt as _, TooltipOverlay, TooltipRequest, TooltipTrigger};

let trigger = TooltipTrigger::new();
div()
    .id("save")
    .on_prepaint({
        let trigger = trigger.clone();
        move |bounds, window, _| trigger.prepaint(bounds, window)
    })
    .on_hover(move |hovered, window, cx| {
        if *hovered {
            overlay.update(cx, |overlay: &mut TooltipOverlay, cx| {
                let request = TooltipRequest::for_trigger(&trigger, |_, cx| {
                    cx.new(|_| SaveTooltip).into()
                });
                overlay.request_show(request, window, cx);
            });
        }
    })
```

A trigger in a view GPUI reuses from the last frame counts as drawn while the
pointer rests on it. `TooltipRequest::new` anchors a request to fixed bounds
instead, and the tooltip stays until it is hidden.

## Complete Rust example

The complete implementation used by the runnable showcase is embedded directly from Rust source:

<<< ../../../crates/base/examples/showcase/components/tooltip.rs{rust}

The command above supplies application initialization, window creation, and shared `BaseShowcase` state.

## Accessibility

Show on focus as well as hover; tooltips supplement names and contain no required controls.

## Notes

Use stable element IDs where accepted. Verify focus, hover, active, selected, disabled, reduced-motion, and high-contrast appearances in the consuming design system.
