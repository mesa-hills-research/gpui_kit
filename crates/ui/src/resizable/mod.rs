use std::ops::Range;

use gpui::{
    Along, App, Axis, Bounds, Context, ElementId, EventEmitter, IsZero, Pixels, Window, px,
};

use crate::PixelsExt;

mod panel;
mod resize_handle;

pub use panel::*;
pub(crate) use resize_handle::*;

pub(crate) const PANEL_MIN_SIZE: Pixels = px(100.);

/// Creates a resizable panel group with horizontal resizing.
pub fn h_resizable(id: impl Into<ElementId>) -> ResizablePanelGroup {
    ResizablePanelGroup::new(id).axis(Axis::Horizontal)
}

/// Creates a resizable panel group with vertical resizing.
pub fn v_resizable(id: impl Into<ElementId>) -> ResizablePanelGroup {
    ResizablePanelGroup::new(id).axis(Axis::Vertical)
}

/// Creates a resizable panel.
pub fn resizable_panel() -> ResizablePanel {
    ResizablePanel::new()
}

/// State for a resizable panel group.
#[derive(Debug, Clone)]
pub struct ResizableState {
    axis: Axis,
    panels: Vec<ResizablePanelState>,
    sizes: Vec<Pixels>,
    pub(crate) resizing_panel_ix: Option<usize>,
    bounds: Bounds<Pixels>,
}

impl Default for ResizableState {
    fn default() -> Self {
        Self {
            axis: Axis::Horizontal,
            panels: Vec::new(),
            sizes: Vec::new(),
            resizing_panel_ix: None,
            bounds: Bounds::default(),
        }
    }
}

impl ResizableState {
    /// Returns the current panel sizes.
    pub fn sizes(&self) -> &Vec<Pixels> {
        &self.sizes
    }

    pub(crate) fn insert_panel(
        &mut self,
        size: Option<Pixels>,
        index: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let panel_state = ResizablePanelState {
            size,
            ..Default::default()
        };

        let size = size.unwrap_or(PANEL_MIN_SIZE);
        let container_size = self.container_size().max(px(1.));
        let total_leftover_size = (container_size - size).max(px(1.));

        for (index, panel) in self.panels.iter_mut().enumerate() {
            let ratio = self.sizes[index] / container_size;
            self.sizes[index] = total_leftover_size * ratio;
            panel.size = Some(self.sizes[index]);
        }

        if let Some(index) = index {
            self.panels.insert(index, panel_state);
            self.sizes.insert(index, size);
        } else {
            self.panels.push(panel_state);
            self.sizes.push(size);
        }

        cx.notify();
    }

    pub(crate) fn sync_panels_count(
        &mut self,
        axis: Axis,
        panels_count: usize,
        cx: &mut Context<Self>,
    ) {
        let mut changed = self.axis != axis;
        self.axis = axis;

        if panels_count > self.panels.len() {
            let difference = panels_count - self.panels.len();

            self.panels
                .extend(vec![ResizablePanelState::default(); difference]);

            self.sizes.extend(vec![PANEL_MIN_SIZE; difference]);
            changed = true;
        }

        if panels_count < self.panels.len() {
            self.panels.truncate(panels_count);
            self.sizes.truncate(panels_count);
            changed = true;
        }

        if changed {
            self.adjust_to_container_size(cx);
        }
    }

    pub(crate) fn update_panel_size(
        &mut self,
        panel_index: usize,
        bounds: Bounds<Pixels>,
        size_range: Range<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let size = bounds.size.along(self.axis);
        let mut changed = false;

        if self.sizes[panel_index].as_f32() == PANEL_MIN_SIZE.as_f32()
            && self.sizes[panel_index] != size
        {
            self.sizes[panel_index] = size;
            self.panels[panel_index].size = Some(size);
            changed = true;
        }

        let panel = &mut self.panels[panel_index];

        if panel.bounds != bounds {
            panel.bounds = bounds;
            changed = true;
        }

        if panel.size_range != size_range {
            panel.size_range = size_range;
            changed = true;
        }

        if changed {
            cx.notify();
        }
    }

    pub(crate) fn remove_panel(&mut self, panel_index: usize, cx: &mut Context<Self>) {
        self.panels.remove(panel_index);
        self.sizes.remove(panel_index);

        if let Some(resizing_panel_index) = self.resizing_panel_ix {
            if resizing_panel_index > panel_index {
                self.resizing_panel_ix = Some(resizing_panel_index - 1);
            }
        }

        self.adjust_to_container_size(cx);
    }

    pub(crate) fn replace_panel(
        &mut self,
        panel_index: usize,
        panel: ResizablePanelState,
        cx: &mut Context<Self>,
    ) {
        let old_size = self.sizes[panel_index];

        self.panels[panel_index] = panel;
        self.sizes[panel_index] = old_size;

        self.adjust_to_container_size(cx);
    }

    pub(crate) fn clear(&mut self) {
        self.panels.clear();
        self.sizes.clear();
    }

    #[inline]
    pub(crate) fn container_size(&self) -> Pixels {
        self.bounds.size.along(self.axis)
    }

    pub(crate) fn done_resizing(&mut self, cx: &mut Context<Self>) {
        self.resizing_panel_ix = None;
        cx.emit(ResizablePanelEvent::Resized);
    }

    fn panel_size_range(&self, index: usize) -> Range<Pixels> {
        self.panels
            .get(index)
            .map(|panel| panel.size_range.clone())
            .unwrap_or(PANEL_MIN_SIZE..Pixels::MAX)
    }

    fn sync_real_panel_sizes(&mut self, _: &App) {
        for (index, panel) in self.panels.iter().enumerate() {
            self.sizes[index] = panel.bounds.size.along(self.axis);
        }
    }

    fn resize_panel(
        &mut self,
        index: usize,
        size: Pixels,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let old_sizes = self.sizes.clone();

        if old_sizes.len() < 2 || index >= old_sizes.len() - 1 {
            return;
        }

        let container_size = self.container_size();
        self.sync_real_panel_sizes(cx);

        let movement = size - old_sizes[index];

        if movement == px(0.) {
            return;
        }

        let main_size_range = self.panel_size_range(index);
        let new_size = size.clamp(main_size_range.start, main_size_range.end);
        let expanding = movement > px(0.);
        let main_index = index;
        let mut cursor_index = index;
        let mut new_sizes = old_sizes.clone();

        if expanding {
            let mut remaining = new_size - old_sizes[index];
            new_sizes[index] = new_size;

            while remaining > px(0.) && cursor_index < old_sizes.len() - 1 {
                cursor_index += 1;

                let size_range = self.panel_size_range(cursor_index);
                let available = (new_sizes[cursor_index] - size_range.start).max(px(0.));
                let reduction = remaining.min(available);

                new_sizes[cursor_index] -= reduction;
                remaining -= reduction;
            }
        } else {
            let mut remaining = new_size - size;
            new_sizes[index] = new_size;

            while remaining > px(0.) && cursor_index > 0 {
                cursor_index -= 1;

                let size_range = self.panel_size_range(cursor_index);
                let available = (new_sizes[cursor_index] - size_range.start).max(px(0.));
                let reduction = remaining.min(available);

                remaining -= reduction;
                new_sizes[cursor_index] -= reduction;
            }

            let adjacent_index = main_index + 1;
            let adjacent_range = self.panel_size_range(adjacent_index);
            let adjacent_growth = old_sizes[main_index] - size - remaining;

            new_sizes[adjacent_index] =
                (new_sizes[adjacent_index] + adjacent_growth).min(adjacent_range.end);
        }

        let total_size: Pixels = new_sizes
            .iter()
            .map(|size| size.as_f32())
            .sum::<f32>()
            .into();

        if total_size > container_size {
            let overflow = total_size - container_size;

            new_sizes[main_index] =
                (new_sizes[main_index] - overflow).max(main_size_range.start);
        }

        for (index, size) in new_sizes.iter().copied().enumerate() {
            self.panels[index].size = Some(size);
        }

        self.sizes = new_sizes;
        cx.notify();
    }

    fn adjust_to_container_size(&mut self, cx: &mut Context<Self>) {
        if self.container_size().is_zero() || self.panels.is_empty() {
            return;
        }

        let container_size = self.container_size();
        let total_size = px(self.sizes.iter().map(|size| size.as_f32()).sum::<f32>());

        if total_size.is_zero() {
            let equal_size = container_size / self.panels.len() as f32;

            for index in 0..self.panels.len() {
                self.sizes[index] = equal_size;
                self.panels[index].size = Some(equal_size);
            }

            cx.notify();
            return;
        }

        for index in 0..self.panels.len() {
            let ratio = self.sizes[index] / total_size;
            let new_size = container_size * ratio;

            self.sizes[index] = new_size;
            self.panels[index].size = Some(new_size);
        }

        cx.notify();
    }
}

impl EventEmitter<ResizablePanelEvent> for ResizableState {}

#[derive(Debug, Clone, Default)]
pub(crate) struct ResizablePanelState {
    pub size: Option<Pixels>,
    pub size_range: Range<Pixels>,
    bounds: Bounds<Pixels>,
}