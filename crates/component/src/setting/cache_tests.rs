use super::*;
use crate::{
    Root,
    setting::{SettingGroup, SettingItem},
};
use gpui::{
    Context, Focusable as _, InteractiveElement as _, Render, TestAppContext, VisualTestContext,
    size,
};
use std::{cell::Cell, rc::Rc};

type Counts = Rc<[Cell<usize>; 3]>;

struct CacheHost {
    input: Entity<InputState>,
    counts: Counts,
    revisions: [u64; 2],
    extra_height: f32,
    state: Option<Entity<SettingsState>>,
}

impl Render for CacheHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let groups = (0..3).map(|ix| {
            let counts = self.counts.clone();
            let input = self.input.clone();
            let extra_height = self.extra_height;
            let group = if ix < 2 {
                SettingGroup::new().key(self.revisions[ix])
            } else {
                SettingGroup::new()
            };
            group
                .item(
                    SettingItem::render(move |_, _, cx| {
                        counts[ix].set(counts[ix].get() + 1);
                        div()
                            .w_full()
                            .h(px(60.
                                + if ix == 0 {
                                    extra_height + input.read(cx).value().len() as f32 * 10.
                                } else {
                                    0.
                                }))
                            .when(ix == 0, |this| this.child(Input::new(&input)))
                            .debug_selector(move || format!("cached-item-{ix}"))
                    })
                    .keywords(["first"]),
                )
                .item(
                    SettingItem::render(|_, _, _| {
                        div()
                            .h_8()
                            .child("Second item")
                            .debug_selector(|| "second-item".into())
                    })
                    .keywords(["second"]),
                )
        });
        let settings = Settings::new("cache-test")
            .page(SettingPage::new("General").resettable(false).groups(groups))
            .render(window, cx)
            .into_any_element();
        self.state = Some(window.use_keyed_state("cache-test", cx, |_, _| unreachable!()));
        settings
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn setup(cx: &mut TestAppContext) -> (Entity<CacheHost>, Counts, &mut VisualTestContext) {
    cx.update(|cx| {
        crate::init(cx);
        crate::Theme::global_mut(cx).font_size = px(16.);
    });
    let counts: Counts = Rc::new(std::array::from_fn(|_| Cell::new(0)));
    let mut host = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| CacheHost {
            input: cx.new(|cx| InputState::new(window, cx)),
            counts: counts.clone(),
            revisions: [0; 2],
            extra_height: 0.,
            state: None,
        });
        host = Some(view.clone());
        Root::new(view, window, cx)
    });
    cx.simulate_resize(size(px(1000.), px(900.)));
    draw(cx);
    draw(cx);
    (host.unwrap(), counts, cx)
}

fn counts(counts: &Counts) -> [usize; 3] {
    std::array::from_fn(|ix| counts[ix].get())
}

#[gpui::test]
fn keyed_groups_skip_unchanged_renders_and_unkeyed_groups_stay_live(cx: &mut TestAppContext) {
    let (host, rendered, cx) = setup(cx);
    let before = counts(&rendered);
    host.update(cx, |_, cx| cx.notify());
    draw(cx);
    let unchanged = counts(&rendered);
    assert_eq!(unchanged[..2], before[..2]);
    assert!(unchanged[2] > before[2]);

    host.update(cx, |host, cx| {
        host.revisions[0] += 1;
        cx.notify();
    });
    draw(cx);
    assert!(rendered[0].get() > unchanged[0]);
    assert_eq!(rendered[1].get(), unchanged[1]);

    let old_top = cx.debug_bounds("cached-item-1").unwrap().top();
    host.update(cx, |host, cx| {
        host.revisions[0] += 1;
        host.extra_height = 80.;
        cx.notify();
    });
    draw(cx);
    assert!(rendered[0].get() > unchanged[0]);
    assert_eq!(
        cx.debug_bounds("cached-item-0").unwrap().size.height,
        px(140.)
    );
    assert_eq!(
        cx.debug_bounds("cached-item-1").unwrap().top() - old_top,
        px(80.)
    );
    // Moving the following group invalidates its cached bounds once.
    draw(cx);
    let settled = counts(&rendered);
    host.update(cx, |_, cx| cx.notify());
    draw(cx);
    assert_eq!(counts(&rendered)[..2], settled[..2]);
}

#[gpui::test]
fn typing_invalidates_only_its_group_and_tracks_growing_content(cx: &mut TestAppContext) {
    let (host, rendered, cx) = setup(cx);
    cx.update(|window, cx| host.read(cx).input.focus_handle(cx).focus(window, cx));
    draw(cx);
    let before = counts(&rendered);
    cx.simulate_input("hello");
    draw(cx);
    draw(cx);
    assert!(rendered[0].get() > before[0]);
    assert_eq!(
        cx.debug_bounds("cached-item-0").unwrap().size.height,
        px(110.)
    );
    let first = cx.debug_bounds("cached-item-0").unwrap();
    let second = cx.debug_bounds("cached-item-1").unwrap();
    assert!(second.top() > first.bottom());
    // After growth has settled, a same-height edit leaves the other group cached.
    let before = counts(&rendered);
    cx.update(|window, cx| {
        host.read(cx)
            .input
            .clone()
            .update(cx, |input, cx| input.set_value("world", window, cx));
    });
    draw(cx);
    assert!(rendered[0].get() > before[0]);
    assert_eq!(rendered[1].get(), before[1]);
}

#[gpui::test]
fn search_resize_and_theme_changes_invalidate_group_caches(cx: &mut TestAppContext) {
    let (host, rendered, cx) = setup(cx);
    cx.update(|window, cx| {
        let state = host.read(cx).state.clone().unwrap();
        let search = state.read(cx).search_input.clone();
        search.update(cx, |input, cx| input.set_value("first", window, cx));
    });
    draw(cx);
    assert!(cx.debug_bounds("cached-item-0").is_some());
    assert!(cx.debug_bounds("second-item").is_none());
    let before = counts(&rendered);
    cx.simulate_resize(size(px(800.), px(900.)));
    draw(cx);
    assert!(rendered[0].get() > before[0]);
    let before = counts(&rendered);
    cx.update(|_, cx| crate::Theme::update(cx, |theme| theme.font_size = px(20.)));
    draw(cx);
    assert!(rendered[0].get() > before[0]);
    assert!(rendered[1].get() > before[1]);
}

#[test]
fn setting_group_key_builder_is_optional() {
    assert_eq!(SettingGroup::new().key, None);
    assert_eq!(SettingGroup::new().key(42).key, Some(42));
}
