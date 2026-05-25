//! # Horizon Tape / Orientation HUD Strip
//!
//! This widget implements a rolling cardinal horizon tape synchronized directly
//! with the device's magnetometer/compass (smooth yaw value). It displays compass
//! tick marks and cardinal direction labels (N, NE, E, SE, S, SW, W, NW) sliding
//! responsively across a horizontal HUD strip.

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::geometry::{Rect, Size};
use agg_gui::text::Font;
use agg_gui::event::{Event, EventResult};
use agg_gui::widget::Widget;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

/// Widget displaying a rolling compass tape HUD.
pub struct HorizonTapeWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    yaw: Rc<Cell<f64>>, // Smoothed device yaw angle in radians (0 = North, PI/2 = East, etc.)
}

impl HorizonTapeWidget {
    /// Create a new rolling horizon tape HUD widget.
    pub fn new(font: Arc<Font>, yaw: Rc<Cell<f64>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            yaw,
        }
    }
}

impl Widget for HorizonTapeWidget {
    fn type_name(&self) -> &'static str {
        "HorizonTapeWidget"
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        // Horizon tape has a fixed height, say 28.0 pixels, but stretches horizontally
        let height = 28.0;
        self.bounds = Rect::new(0.0, 0.0, available.width, height);
        Size::new(available.width, height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let b = self.bounds;
        let w = b.width;
        let h = b.height;

        // Draw a dark semi-transparent background for the tape
        let bg_color = Color::from_rgba8(20, 20, 30, 200);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.set_fill_color(bg_color);
        ctx.fill();

        // Draw top and bottom borders
        let border_color = Color::from_rgba8(255, 255, 255, 40);
        ctx.begin_path();
        ctx.move_to(0.0, h);
        ctx.line_to(w, h);
        ctx.set_stroke_color(border_color);
        ctx.set_line_width(1.0);
        ctx.stroke();

        ctx.begin_path();
        ctx.move_to(0.0, 0.0);
        ctx.line_to(w, 0.0);
        ctx.set_stroke_color(border_color);
        ctx.set_line_width(1.0);
        ctx.stroke();

        // Draw a center indicator pointer (little red tick pointing down/up)
        let center_x = w / 2.0;
        let indicator_color = Color::from_rgb8(255, 100, 100);
        ctx.begin_path();
        ctx.move_to(center_x, h);
        ctx.line_to(center_x, h - 6.0);
        ctx.set_stroke_color(indicator_color);
        ctx.set_line_width(2.0);
        ctx.stroke();

        ctx.begin_path();
        ctx.move_to(center_x, 0.0);
        ctx.line_to(center_x, 6.0);
        ctx.set_stroke_color(indicator_color);
        ctx.set_line_width(2.0);
        ctx.stroke();

        // Yaw angle in degrees: 0 to 360
        let yaw_deg = self.yaw.get().to_degrees();

        // 1 degree = N pixels on screen. Let's say 4 pixels per degree.
        let pixels_per_degree = 4.0;

        // Draw compass marks from yaw_deg - visible_half to yaw_deg + visible_half
        let half_visible_deg = (center_x / pixels_per_degree) as i32;

        let start_deg = (yaw_deg as i32 - half_visible_deg - 5).max(-360);
        let end_deg = (yaw_deg as i32 + half_visible_deg + 5).min(720);

        ctx.set_font(Arc::clone(&self.font));

        for deg in start_deg..=end_deg {
            // Normalized degree to 0..360 range
            let norm_deg = (deg % 360 + 360) % 360;

            // Offset of this degree from the current yaw center
            let deg_diff = deg as f64 - yaw_deg;
            let tick_x = center_x + deg_diff * pixels_per_degree;

            // Only draw if inside tape boundaries
            if tick_x >= 0.0 && tick_x <= w {
                let is_major = norm_deg % 30 == 0;
                let is_cardinal = norm_deg % 45 == 0;

                let line_color = Color::from_rgba8(255, 255, 255, 150);

                if is_cardinal {
                    // Draw a strong major tick
                    ctx.begin_path();
                    ctx.move_to(tick_x, 0.0);
                    ctx.line_to(tick_x, 10.0);
                    ctx.set_stroke_color(line_color);
                    ctx.set_line_width(1.5);
                    ctx.stroke();

                    ctx.begin_path();
                    ctx.move_to(tick_x, h);
                    ctx.line_to(tick_x, h - 10.0);
                    ctx.set_stroke_color(line_color);
                    ctx.set_line_width(1.5);
                    ctx.stroke();

                    // Get cardinal label
                    let label = match norm_deg {
                        0 => "N",
                        45 => "NE",
                        90 => "E",
                        135 => "SE",
                        180 => "S",
                        225 => "SW",
                        270 => "W",
                        315 => "NW",
                        _ => "",
                    };

                    ctx.set_font_size(11.0);
                    let label_color = if norm_deg == 0 {
                        Color::from_rgb8(255, 100, 100) // Red for North!
                    } else {
                        Color::from_rgb8(220, 220, 220)
                    };

                    // Draw the text in the middle of the tape
                    let text_width = label.len() as f64 * 7.0; // simple estimation
                    ctx.set_fill_color(label_color);
                    ctx.fill_text(
                        label,
                        tick_x - text_width / 2.0,
                        h / 2.0 - 4.0
                    );
                } else if is_major {
                    // Draw medium tick
                    ctx.begin_path();
                    ctx.move_to(tick_x, 0.0);
                    ctx.line_to(tick_x, 6.0);
                    ctx.set_stroke_color(line_color);
                    ctx.set_line_width(1.0);
                    ctx.stroke();

                    ctx.begin_path();
                    ctx.move_to(tick_x, h);
                    ctx.line_to(tick_x, h - 6.0);
                    ctx.set_stroke_color(line_color);
                    ctx.set_line_width(1.0);
                    ctx.stroke();

                    // Draw numerical degree label (e.g. 30, 60, 120, etc.)
                    let deg_str = format!("{}", norm_deg);
                    ctx.set_font_size(8.0);
                    let label_color = Color::from_rgba8(255, 255, 255, 100);
                    let text_width = deg_str.len() as f64 * 5.0;
                    ctx.set_fill_color(label_color);
                    ctx.fill_text(
                        &deg_str,
                        tick_x - text_width / 2.0,
                        h / 2.0 - 3.0
                    );
                } else if norm_deg % 5 == 0 {
                    // Draw minor tick
                    let minor_color = Color::from_rgba8(255, 255, 255, 60);
                    ctx.begin_path();
                    ctx.move_to(tick_x, 0.0);
                    ctx.line_to(tick_x, 4.0);
                    ctx.set_stroke_color(minor_color);
                    ctx.set_line_width(0.8);
                    ctx.stroke();

                    ctx.begin_path();
                    ctx.move_to(tick_x, h);
                    ctx.line_to(tick_x, h - 4.0);
                    ctx.set_stroke_color(minor_color);
                    ctx.set_line_width(0.8);
                    ctx.stroke();
                }
            }
        }
    }
}
