//! # Wrapping Row Container
//!
//! Greedy left-to-right row that wraps onto additional rows when the
//! children don't fit on a single line. Used by the configuration
//! tray on narrow viewports (mobile portrait) so the row doesn't
//! overflow the visible width.
//!
//! Mirrors `agg_gui::widgets::FlexRow`'s ergonomics — `add()` builder
//! method, `with_gap()`, integer-snapped child bounds — without the
//! flex factor / cross-axis anchoring features (we don't need them
//! for the bottom bar).

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult};
use agg_gui::geometry::{Rect, Size};
use agg_gui::widget::Widget;

/// A row of widgets that wraps onto new rows when children would
/// otherwise overflow the available width.
pub struct WrappingRow {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    /// Horizontal gap between siblings on the same row.
    h_gap: f64,
    /// Vertical gap between rows when wrapping occurs.
    v_gap: f64,
    /// Background colour, only painted if alpha > 0.
    background: Color,
}

impl Default for WrappingRow {
    fn default() -> Self {
        Self::new()
    }
}

impl WrappingRow {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            h_gap: 8.0,
            v_gap: 6.0,
            background: Color::from_rgba8(0, 0, 0, 0),
        }
    }

    pub fn with_gap(mut self, h_gap: f64, v_gap: f64) -> Self {
        self.h_gap = h_gap;
        self.v_gap = v_gap;
        self
    }

    pub fn with_background(mut self, color: Color) -> Self {
        self.background = color;
        self
    }

    pub fn add(mut self, child: Box<dyn Widget>) -> Self {
        self.children.push(child);
        self
    }
}

impl Widget for WrappingRow {
    fn type_name(&self) -> &'static str {
        "WrappingRow"
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
        let n = self.children.len();
        if n == 0 {
            self.bounds = Rect::new(0.0, 0.0, available.width, 0.0);
            return Size::new(available.width, 0.0);
        }

        // Step 1: measure each child's natural width.
        let mut content_sizes: Vec<Size> = Vec::with_capacity(n);
        for child in self.children.iter_mut() {
            let s = child.layout(Size::new(available.width, available.height));
            content_sizes.push(s);
        }

        // Step 2: greedy pack left-to-right, wrapping when the next
        // child would overflow. Track row index + x offset per child.
        let mut row_assignments: Vec<(usize, f64, f64)> = Vec::with_capacity(n); // (row_idx, x, row_h)
        let mut row_heights: Vec<f64> = Vec::new();
        let mut cur_row = 0_usize;
        let mut cur_x = 0.0_f64;
        let mut cur_row_h = 0.0_f64;
        for s in content_sizes.iter() {
            let needs_gap_before = cur_x > 0.0;
            let prospective_x = if needs_gap_before {
                cur_x + self.h_gap
            } else {
                cur_x
            };
            let prospective_right = prospective_x + s.width;
            if prospective_right > available.width && cur_x > 0.0 {
                // Wrap to new row. Commit current row's height.
                row_heights.push(cur_row_h);
                cur_row += 1;
                cur_x = 0.0;
                cur_row_h = 0.0;
            }
            let placed_x = if cur_x > 0.0 {
                cur_x + self.h_gap
            } else {
                0.0
            };
            row_assignments.push((cur_row, placed_x, 0.0)); // row_h filled in after this row is closed
            cur_x = placed_x + s.width;
            cur_row_h = cur_row_h.max(s.height);
        }
        // Close the final row.
        row_heights.push(cur_row_h);

        // Step 3: compute Y offset for each row (cumulative + v_gap),
        // and assign bounds. We paint with Y-up coordinates so the
        // FIRST row should sit at the TOP — meaning higher y values.
        // Total content height first, then each row's top y = total -
        // sum(prev_rows + gaps) - this_row's height. Mirrors the
        // pattern used by FlexColumn in agg-gui.
        let total_h: f64 = row_heights.iter().copied().sum::<f64>()
            + self.v_gap * row_heights.len().saturating_sub(1) as f64;

        // Y offset for each row (top of each row in widget-local
        // coords, Y-up so y=0 is the BOTTOM of the row container,
        // y=total_h is the TOP).
        let mut row_top_y: Vec<f64> = Vec::with_capacity(row_heights.len());
        let mut y_cursor = total_h;
        for h in &row_heights {
            row_top_y.push(y_cursor);
            y_cursor -= h + self.v_gap;
        }

        for (i, child) in self.children.iter_mut().enumerate() {
            let (row_idx, x, _) = row_assignments[i];
            let s = content_sizes[i];
            // Y-up: child bounds y is the bottom of the slot.
            let top_y = row_top_y[row_idx];
            let bottom_y = top_y - row_heights[row_idx];
            // Centre the child vertically within its row if shorter
            // than the row height.
            let pad = (row_heights[row_idx] - s.height).max(0.0) * 0.5;
            let child_bottom = bottom_y + pad;
            child.set_bounds(Rect::new(
                x.round(),
                child_bottom.round(),
                s.width.round(),
                s.height.round(),
            ));
        }

        self.bounds = Rect::new(0.0, 0.0, available.width, total_h);
        Size::new(available.width, total_h)
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if self.background.a > 0.001 {
            ctx.set_fill_color(self.background);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
            ctx.fill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agg_gui::geometry::Point;
    use agg_gui::widget::Widget;

    /// Minimal child widget for layout tests — reports a fixed natural
    /// size when laid out and remembers the bounds the parent assigns
    /// it. Lets us verify the row both wraps correctly AND positions
    /// each child where we expect.
    struct Probe {
        size: Size,
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
    }

    impl Probe {
        fn new(w: f64, h: f64) -> Self {
            Self {
                size: Size::new(w, h),
                bounds: Rect::default(),
                children: Vec::new(),
            }
        }
    }

    impl Widget for Probe {
        fn type_name(&self) -> &'static str {
            "Probe"
        }
        fn bounds(&self) -> Rect {
            self.bounds
        }
        fn set_bounds(&mut self, b: Rect) {
            self.bounds = b;
        }
        fn children(&self) -> &[Box<dyn Widget>] {
            &self.children
        }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
            &mut self.children
        }
        fn layout(&mut self, _available: Size) -> Size {
            self.size
        }
        fn on_event(&mut self, _: &Event) -> EventResult {
            EventResult::Ignored
        }
        fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
        fn hit_test(&self, _: Point) -> bool {
            false
        }
    }

    #[test]
    fn fits_single_row_when_room_available() {
        // Three 50-px children with 8-px gaps = 50+8+50+8+50 = 166 px.
        // Available width 500 → all on one row.
        let mut row = WrappingRow::new()
            .with_gap(8.0, 6.0)
            .add(Box::new(Probe::new(50.0, 30.0)))
            .add(Box::new(Probe::new(50.0, 30.0)))
            .add(Box::new(Probe::new(50.0, 30.0)));
        let s = row.layout(Size::new(500.0, 100.0));
        assert_eq!(s.height as i64, 30, "single-row height = tallest child");
        // All three on same Y.
        let ys: Vec<f64> = row
            .children()
            .iter()
            .map(|c| c.bounds().y)
            .collect();
        assert_eq!(ys[0], ys[1]);
        assert_eq!(ys[1], ys[2]);
        // Xs strictly increasing.
        assert!(row.children()[0].bounds().x < row.children()[1].bounds().x);
        assert!(row.children()[1].bounds().x < row.children()[2].bounds().x);
    }

    #[test]
    fn wraps_when_overflow() {
        // Four 60-px children + 8-px gaps. Available 150 px → only
        // one fits per "row" if you require 60+gap+60 > 150... Hmm
        // 60+8+60 = 128 ≤ 150 so two fit per row. 4 → 2 rows of 2.
        let mut row = WrappingRow::new()
            .with_gap(8.0, 6.0)
            .add(Box::new(Probe::new(60.0, 20.0)))
            .add(Box::new(Probe::new(60.0, 20.0)))
            .add(Box::new(Probe::new(60.0, 20.0)))
            .add(Box::new(Probe::new(60.0, 20.0)));
        let s = row.layout(Size::new(150.0, 200.0));
        // Two rows of 20 + 6-px vertical gap = 46.
        assert_eq!(s.height as i64, 46, "expected 2 rows, got h={}", s.height);
        let ys: Vec<f64> = row
            .children()
            .iter()
            .map(|c| c.bounds().y)
            .collect();
        assert_eq!(ys[0], ys[1], "first two on same row");
        assert_eq!(ys[2], ys[3], "last two on same row");
        assert!(ys[0] != ys[2], "rows must be on different Ys");
    }
}
