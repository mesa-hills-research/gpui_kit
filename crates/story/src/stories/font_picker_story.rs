use gpui_kit::component::{
    button::Button,
    checkbox::Checkbox,
    font_picker::{FontCatalog, FontPicker, FontPickerState},
    h_flex, v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, IntoElement, ParentElement as _,
    Render, Styled as _, Task, Window, prelude::FluentBuilder as _,
};

use crate::section;

pub struct FontPickerStory {
    focus_handle: FocusHandle,
    picker: Entity<FontPickerState>,
    compact: bool,
    load: Task<()>,
}

impl super::Story for FontPickerStory {
    fn title() -> &'static str {
        "FontPicker"
    }

    fn description() -> &'static str {
        "Search installed fonts while their catalog loads in the background."
    }

    fn new_view(window: &mut Window, cx: &mut App) -> Entity<impl Render> {
        cx.new(|cx| Self {
            focus_handle: cx.focus_handle(),
            picker: cx.new(|cx| FontPickerState::new(window, cx)),
            compact: false,
            load: Task::ready(()),
        })
    }
}

impl Focusable for FontPickerStory {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for FontPickerStory {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w_full()
            .gap_4()
            .child(
                h_flex()
                    .gap_4()
                    .child(
                        Checkbox::new("compact")
                            .label("Compact layout")
                            .checked(self.compact)
                            .on_click(cx.listener(|this, checked, _, cx| {
                                this.compact = *checked;
                                cx.notify();
                            })),
                    )
                    .child(Button::new("reload-fonts").label("Reload fonts").on_click(
                        cx.listener(|this, _, window, cx| {
                            let load = FontCatalog::load(cx);
                            this.load = cx.spawn_in(window, async move |this, cx| {
                                let catalog = load.await;
                                this.update_in(cx, |this, window, cx| {
                                    this.picker.update(cx, |picker, cx| {
                                        picker.set_catalog(catalog, window, cx);
                                    });
                                })
                                .ok();
                            });
                        }),
                    )),
            )
            .child(
                section("Installed fonts")
                    .description(
                        "Reload the catalog with the search or dropdown open to keep your place.",
                    )
                    .child(FontPicker::new(&self.picker).when(self.compact, FontPicker::compact)),
            )
    }
}
