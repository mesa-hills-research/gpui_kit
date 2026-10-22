//! What the text view memory bench and test share, a count of the live heap
//! and the Markdown they measure.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    fmt::Write as _,
    sync::atomic::{AtomicIsize, Ordering},
};

/// Counts the bytes allocated and not yet freed.
struct LiveBytes;

static LIVE: AtomicIsize = AtomicIsize::new(0);

// SAFETY: every operation is forwarded unchanged to the system allocator. The
// counter has no bearing on allocation validity or lifetime.
unsafe impl GlobalAlloc for LiveBytes {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed);
        // SAFETY: `layout` is passed through from the caller.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        // SAFETY: `ptr` and `layout` are the pair the caller allocated.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(
            new_size as isize - layout.size() as isize,
            Ordering::Relaxed,
        );
        // SAFETY: all arguments are passed through from the caller.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: LiveBytes = LiveBytes;

pub fn live() -> isize {
    LIVE.load(Ordering::Relaxed)
}

/// An answer of about 4 KB: prose with emphasis, links and inline code, a
/// list, and a code block in every third answer.
pub fn answer(ix: usize) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "## Answer {ix}: what the data shows\n");
    for paragraph in 0..5 {
        let _ = writeln!(
            out,
            "Paragraph {paragraph} of answer {ix}. The **dip window** is `08-18` → `08-26` \
             (trough $208.48 on 08-24), followed by a post-earnings jump. Let me confirm the \
             earnings date and date the specific news events in that window, then weave in \
             [the filing](https://example.com/filing/{ix}) and the *guidance* update so the \
             numbers line up with what the market priced in before the open.\n"
        );
    }
    let _ = writeln!(out, "- FOMC 09-16: +25bp to 3.75–4.00%, 12-0.");
    let _ = writeln!(out, "- Dot plot: one more hike implied this year.");
    let _ = writeln!(out, "- XLE +45.2% YTD, the standout winner.\n");
    if ix % 3 == 0 {
        let _ = writeln!(
            out,
            "```rust\nfn main() {{\n    let total: u64 = (0..{ix}).sum();\n    println!(\"{{total}}\");\n}}\n```\n"
        );
    }
    out
}
