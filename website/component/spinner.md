---
title: Spinner
description: Displays an animated loading showing the completion progress of a task.
---

# Spinner

Spinner element displays an animated loading. Perfect for showing loading states, progress spinners, and other visual feedback during asynchronous operations. Features customizable icons, colors, sizes, and rotation animations.

## Import

```rust
use gpui_kit::component::spinner::Spinner;
```

## Usage

### Basic

```rust
// Default loader icon
Spinner::new()
```

### Spinner with Custom Color

```rust
use gpui_kit::component::ActiveTheme;

// Blue spinner
Spinner::new()
    .color(cx.theme().blue)

// Green spinner for success states
Spinner::new()
    .color(cx.theme().green)

// Custom color
Spinner::new()
    .color(cx.theme().cyan)
```

### Spinner Sizes

```rust
// Extra small spinner
Spinner::new().xsmall()

// Small spinner
Spinner::new().small()

// Medium spinner (default)
Spinner::new()

// Large spinner
Spinner::new().large()

// Custom size
Spinner::new().with_size(px(64.))
```

### Spinner with Custom Icon

```rust
use gpui_kit::component::IconName;

// Loading circle icon
Spinner::new()
    .icon(IconName::LoaderCircle)

// Large loading circle with custom color
Spinner::new()
    .icon(IconName::LoaderCircle)
    .large()
    .color(cx.theme().cyan)

// Different loading icons
Spinner::new()
    .icon(IconName::Loader)
    .color(cx.theme().primary)
```

## Available Icons

The Spinner component supports various loading and progress icons:

### Loading Icons

- `Loader` (default) - Rotating line spinner
- `LoaderCircle` - Circular loading spinner

### Other Compatible Icons

- Any icon from the `IconName` enum can be used, though loading-specific icons work best with the rotation animation

## Animation

The spinner turns once every 0.8 seconds at a constant speed, in a frame for each frame the display shows. It follows the app's shared clock, so spinners that show together turn together, and it holds still when the system asks for reduced motion.

- `.ease(curve)` sets the turning curve. The default is `linear`, and any other curve makes the spinner speed up and slow down once in each turn.
- `.max_fps(n)` draws at most `n` frames a second. A cap below the display's rate draws on a timer, so some frames stay up longer than others.

```rust
// Turn at 30 frames a second
Spinner::new().max_fps(30.)
```

The default icon has eight spokes alike, so a step of more than 22.5 degrees in one frame looks like a step backwards. With that icon, draw at least 20 frames a second.

The spinner turns as it paints, so the view it is in is not built again for each step, and a view that the app keeps stays kept.

## Size Reference

| Size        | Method              | Approximate Pixels |
| ----------- | ------------------- | ------------------ |
| Extra Small | `.xsmall()`         | ~12px              |
| Small       | `.small()`          | ~14px              |
| Medium      | (default)           | ~16px              |
| Large       | `.large()`          | ~24px              |
| Custom      | `.with_size(px(n))` | n px               |

## Examples

### Loading States

```rust
// Simple loading spinner
Spinner::new()

// Loading with custom color
Spinner::new()
    .color(cx.theme().blue)

// Large loading spinner
Spinner::new()
    .large()
    .color(cx.theme().primary)
```

### Different Loading Icons

```rust
// Default loader (line spinner)
Spinner::new()
    .color(cx.theme().muted_foreground)

// Circle loader
Spinner::new()
    .icon(IconName::LoaderCircle)
    .color(cx.theme().blue)

// Large circle loader with custom color
Spinner::new()
    .icon(IconName::LoaderCircle)
    .large()
    .color(cx.theme().green)
```

### Status Spinners

```rust
// Loading state
Spinner::new()
    .small()
    .color(cx.theme().muted_foreground)

// Processing state
Spinner::new()
    .icon(IconName::LoaderCircle)
    .color(cx.theme().blue)

// Success processing (still animating)
Spinner::new()
    .icon(IconName::LoaderCircle)
    .color(cx.theme().green)
```

### Size Variations

```rust
// Extra small for inline text
Spinner::new()
    .xsmall()
    .color(cx.theme().muted_foreground)

// Small for buttons
Spinner::new()
    .small()
    .color(cx.theme().primary_foreground)

// Medium for general use (default)
Spinner::new()
    .color(cx.theme().primary)

// Large for prominent loading states
Spinner::new()
    .large()
    .color(cx.theme().blue)

// Custom size for specific requirements
Spinner::new()
    .with_size(px(32.))
    .color(cx.theme().orange)
```

### In UI Components

```rust
// In a button
Button::new("submit-btn")
    .loading(true)
    .icon(
        Spinner::new()
            .small()
            .color(cx.theme().primary_foreground)
    )
    .label("Loading...")

// In a card header
div()
    .flex()
    .items_center()
    .gap_2()
    .child("Processing...")
    .child(
        Spinner::new()
            .small()
            .color(cx.theme().muted_foreground)
    )

// Full-screen loading
div()
    .flex()
    .items_center()
    .justify_center()
    .h_full()
    .w_full()
    .child(
        Spinner::new()
            .large()
            .color(cx.theme().primary)
    )
```

## Performance Considerations

- The spinner turns as it paints, so it costs little to draw in each frame
- Multiple spinners on the same page share the same animation timing
- The component is lightweight and suitable for frequent updates
- Consider using smaller sizes for better performance with many spinners

## Common Patterns

### Conditional Loading

```rust
// Show spinner only when loading
.when(is_loading, |this| {
    this.child(
        Spinner::new()
            .small()
            .color(cx.theme().muted_foreground)
    )
})
```

### Loading with Text

```rust
// Loading text with spinner
h_flex()
    .items_center()
    .gap_2()
    .child(
        Spinner::new()
            .small()
            .color(cx.theme().primary)
    )
    .child("Loading data...")
```

### Overlay Loading

```rust
// Full overlay with spinner
div()
    .absolute()
    .inset_0()
    .flex()
    .items_center()
    .justify_center()
    .bg(cx.theme().background.alpha(0.8))
    .child(
        v_flex()
            .items_center()
            .gap_3()
            .child(
                Spinner::new()
                    .large()
                    .color(cx.theme().primary)
            )
            .child("Loading...")
    )
```
