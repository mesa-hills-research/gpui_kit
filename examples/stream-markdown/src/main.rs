use std::{cell::RefCell, rc::Rc};

use gpui_kit::assets::Assets;
use gpui_kit::component::{
    button::Button,
    h_flex,
    message_scroller::{MessageScroller, MessageScrollerState},
    switch::Switch,
    text::{TextBlockId, TextView, TextViewState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub struct Example {
    markdown_state: Entity<TextViewState>,
    tx: smol::channel::Sender<(usize, String)>,
    scroll_handle: ScrollHandle,
    replay_id: usize,
    stream_fade: bool,
    /// Shows each top-level block as a row of a list, the way a chat
    /// transcript keeps a long answer cheap to draw.
    as_rows: bool,
    /// Shows the first lines of each code block only.
    cap_code_blocks: bool,
    scroller: Entity<MessageScrollerState>,
    /// One view per block of the answer, and the block each shows.
    rows: Rc<RefCell<Vec<(TextBlockId, Entity<TextViewState>)>>>,
    _sync_rows: Subscription,
    _task: Task<()>,
    _update_task: Task<()>,
}

const EXAMPLE: &str = include_str!("../../fixtures/test.md");

impl Example {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let markdown_state =
            cx.new(|cx| TextViewState::markdown("# Streaming Markdown Parse\n\n", cx));
        let scroll_handle = ScrollHandle::new();
        let scroller = cx.new(|cx| MessageScrollerState::new(0, cx));

        let (tx, rx) = smol::channel::unbounded::<(usize, String)>();
        let _task = cx.spawn({
            async move |weak_self, cx| {
                while let Ok((replay_id, chunk)) = rx.recv().await {
                    _ = weak_self.update(cx, |this, cx| {
                        if replay_id != this.replay_id {
                            return;
                        }

                        // Push the new chunk to the markdown state,
                        // it will reparse and re-render automatically.
                        this.markdown_state.update(cx, |state, cx| {
                            state.push_str(&chunk, cx);
                        });
                        this.scroll_handle.scroll_to_bottom();
                    });
                }
            }
        });
        let _sync_rows = cx.observe(&markdown_state, |this, state, cx| {
            this.sync_rows(&state, cx);
        });

        let mut this = Self {
            markdown_state: markdown_state.clone(),
            scroll_handle,
            tx,
            replay_id: 0,
            stream_fade: true,
            as_rows: false,
            cap_code_blocks: false,
            scroller,
            rows: Rc::default(),
            _sync_rows,
            _task,
            _update_task: Task::ready(()),
        };
        this.sync_rows(&markdown_state, cx);
        this
    }

    fn view(window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self::new(window, cx))
    }

    /// Gives each top-level block of the answer a row: a view of that block,
    /// added when the block appears. Ids change when the text is replaced,
    /// and the rows start over then.
    fn sync_rows(&mut self, state: &Entity<TextViewState>, cx: &mut Context<Self>) {
        let blocks = state.read(cx).blocks();
        let mut rows = self.rows.borrow_mut();
        let kept = rows
            .iter()
            .zip(&blocks)
            .take_while(|((id, _), block)| *id == block.id())
            .count();
        let removed = rows.len() - kept;
        rows.truncate(kept);
        for block in &blocks[kept..] {
            let ix = block.index();
            let row = cx.new(|cx| TextViewState::block_range(state, ix..ix + 1, cx));
            rows.push((block.id(), row));
        }
        let added = blocks.len() - kept;
        if removed > 0 || added > 0 {
            self.scroller.update(cx, |scroller, cx| {
                scroller.splice(kept..kept + removed, added, cx);
            });
        }
    }

    /// Simulate streaming by updating markdown state in chunks
    /// 50ms for a iteration, every time adding about 5 - 20 characters
    /// This is just for demonstration; in a real app, you'd stream from a source.
    fn replay(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.replay_id = self.replay_id.wrapping_add(1);
        let replay_id = self.replay_id;
        let tx = self.tx.clone();
        let mut current = 0;
        self.markdown_state.update(cx, |state, cx| {
            state.set_text("", cx);
        });

        self._update_task = cx.background_executor().spawn(async move {
            let chars: Vec<char> = EXAMPLE.chars().collect();
            while current < chars.len() {
                let chunk_size = (5 + rand::random::<usize>() % 15).min(chars.len() - current);
                let chunk: String = chars[current..current + chunk_size].iter().collect();
                _ = tx.try_send((replay_id, chunk));
                current += chunk_size;
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
    }
}

impl Render for Example {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let stream_fade = self.stream_fade;
        let cap_code_blocks = self.cap_code_blocks;
        let rows = self.rows.clone();
        v_flex()
            .id("example")
            .size_full()
            .p_4()
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .child(
                        Button::new("replay")
                            .outline()
                            .label("Replay")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.replay(window, cx);
                            })),
                    )
                    .child(
                        Switch::new("stream-fade")
                            .checked(self.stream_fade)
                            .label("Fade in streamed text")
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.stream_fade = *checked;
                                cx.notify();
                            })),
                    )
                    .child(
                        Switch::new("as-rows")
                            .checked(self.as_rows)
                            .label("A row per block")
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.as_rows = *checked;
                                cx.notify();
                            })),
                    )
                    .child(
                        Switch::new("cap-code-blocks")
                            .checked(self.cap_code_blocks)
                            .label("First 8 lines of code")
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.cap_code_blocks = *checked;
                                cx.notify();
                            })),
                    ),
            )
            .map(|this| {
                if self.as_rows {
                    this.child(
                        MessageScroller::new("rows", self.scroller.clone(), move |ix, _, _| {
                            let row = rows.borrow()[ix].1.clone();
                            div().w_full().child(
                                TextView::new(&row)
                                    .selectable(true)
                                    .stream_fade(stream_fade)
                                    .when(cap_code_blocks, |view| view.code_block_max_lines(8)),
                            )
                        })
                        .flex_1()
                        .w_full(),
                    )
                } else {
                    this.child(
                        div()
                            .id("contents")
                            .flex_1()
                            .w_full()
                            .track_scroll(&self.scroll_handle)
                            .overflow_y_scroll()
                            .size_full()
                            .child(
                                TextView::new(&self.markdown_state)
                                    .selectable(true)
                                    .stream_fade(stream_fade)
                                    .when(cap_code_blocks, |view| view.code_block_max_lines(8)),
                            ),
                    )
                }
            })
    }
}

fn main() {
    let app = gpui_kit::application().with_assets(Assets);

    app.run(move |cx| {
        gpui_component_story::init(cx);
        cx.activate(true);

        gpui_component_story::create_new_window_with_size(
            "Stream Markdown",
            Some(size(px(600.), px(800.))),
            Example::view,
            cx,
        );
    });
}
