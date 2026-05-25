//! # Dynamic Label Widget
//!
//! A custom widget that wraps a standard `Label` and evaluates a text callback
//! closure dynamically on every layout and paint cycle.

use std::sync::Arc;

use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult};
use agg_gui::geometry::{Rect, Size};
use agg_gui::text::Font;
use agg_gui::widget::Widget;
use agg_gui::widgets::Label;

pub struct DynamicLabel {
    label: Label,
    callback: Box<dyn Fn() -> String>,
}

impl DynamicLabel {
    /// Create a new dynamic label.
    pub fn new(callback: impl Fn() -> String + 'static, font: Arc<Font>) -> Self {
        Self {
            label: Label::new("", font),
            callback: Box::new(callback),
        }
    }

    /// Set font size on the wrapped label.
    pub fn with_font_size(mut self, size: f64) -> Self {
        self.label = self.label.with_font_size(size);
        self
    }
}

impl Widget for DynamicLabel {
    fn type_name(&self) -> &'static str {
        "DynamicLabel"
    }

    fn bounds(&self) -> Rect {
        self.label.bounds()
    }

    fn set_bounds(&mut self, b: Rect) {
        self.label.set_bounds(b);
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        self.label.children()
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        self.label.children_mut()
    }

    fn layout(&mut self, available: Size) -> Size {
        let text = (self.callback)();
        self.label.set_text(text);
        self.label.layout(available)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let text = (self.callback)();
        self.label.set_text(text);
        self.label.paint(ctx);
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        self.label.on_event(event)
    }
}
