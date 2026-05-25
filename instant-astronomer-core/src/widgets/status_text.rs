//! # Status Text Widget
//!
//! Minimal text widget that recomputes its label every paint by calling a
//! supplied closure. agg-gui's stock [`Label`](agg_gui::widgets::Label) is
//! optimised for static text + backbuffer caching; this widget is the
//! short-term escape hatch for the per-frame readouts the configuration tray
//! needs ("Lat: …", "Located: …"), all rendered through agg-gui's `DrawCtx`.

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult};
use agg_gui::geometry::{Rect, Size};
use agg_gui::text::Font;
use agg_gui::widget::Widget;
use std::sync::Arc;

/// Live-updating single-line text widget.
pub struct StatusText {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    font_size: f64,
    color: Color,
    text_fn: Box<dyn FnMut() -> String>,
    last_text: String,
}

impl StatusText {
    pub fn new(font: Arc<Font>, text_fn: impl FnMut() -> String + 'static) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            font_size: 12.0,
            color: Color::from_rgb8(220, 220, 230),
            text_fn: Box::new(text_fn),
            last_text: String::new(),
        }
    }

    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
}

impl Widget for StatusText {
    fn type_name(&self) -> &'static str {
        "StatusText"
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        // Reserve enough vertical space for a single line plus a little padding.
        let h = (self.font_size + 6.0).min(available.height.max(self.font_size + 6.0));
        // Measure the *actual* text so a `StatusText` placed as a fixed
        // child of a FlexRow reports its real natural width instead of
        // greedily claiming `available.width`. The previous greedy code
        // pushed sibling flex children to width zero and let `FlexRow`'s
        // cursor advance past the row, causing top-bar widgets to paint
        // on top of each other.
        let text = (self.text_fn)();
        self.last_text = text;
        let metrics =
            agg_gui::text::measure_text_metrics(&self.font, &self.last_text, self.font_size);
        let w = metrics.width.min(available.width);
        self.bounds = Rect::new(0.0, 0.0, w, h);
        Size::new(w, h)
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let text = (self.text_fn)();
        // Track whether the produced text changed; if so, schedule another
        // draw so callers depending on this for live readouts don't stall.
        if text != self.last_text {
            self.last_text = text.clone();
            agg_gui::animation::request_draw_without_invalidation();
        }
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        ctx.set_fill_color(self.color);
        // Clip so a stale `last_text` width estimate (or a parent that
        // ignored our requested size) can't bleed glyphs into the
        // adjacent widget — top-bar overlap regression.
        ctx.save();
        ctx.clip_rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        // Y-up: place baseline a couple of pixels above the bottom of the row.
        ctx.fill_text(&text, 0.0, 4.0);
        ctx.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_default_font;

    /// `StatusText::layout` must report the *measured* text width, not the
    /// full `available.width`. The regression: when this widget was used as
    /// a fixed-width child of a `FlexRow`, it claimed the entire row width
    /// for itself, starved sibling flex children, and the cursor advanced
    /// past the visible row so the next sibling ended up painted on top of
    /// us.
    #[test]
    fn layout_width_tracks_measured_text_not_available_width() {
        let font = load_default_font();
        let mut w = StatusText::new(font, || "Lat: 39.7892°  Lng: -104.9903°".to_string());
        let size = w.layout(Size::new(2000.0, 30.0));
        assert!(
            size.width < 600.0,
            "StatusText should report measured text width, got {} for ~30-char string",
            size.width
        );
        assert!(
            size.width > 0.0,
            "StatusText should report a positive width for non-empty text"
        );
    }

    /// Empty text must still have a non-degenerate height so a row built
    /// from `StatusText`s doesn't collapse.
    #[test]
    fn layout_empty_text_keeps_row_height() {
        let font = load_default_font();
        let mut w = StatusText::new(font, String::new);
        let size = w.layout(Size::new(800.0, 30.0));
        assert!(size.height > 0.0);
    }
}
