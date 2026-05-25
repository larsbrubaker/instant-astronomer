//! # Horizon Tape (Compass HUD)
//!
//! Rolling cardinal-direction strip rendered directly above the control
//! panel. The tape's position is driven by the smoothed device yaw, so as
//! the user pans (mouse drag) or rotates the phone, the cardinal labels
//! slide past a fixed centre indicator. Section 2 of `implementation.md`
//! calls this out as the "HUD Horizon Tape" between the sky viewport and
//! the configuration tray.
//!
//! All rendering uses agg-gui's [`DrawCtx`] — no canvas / WebGL paths.

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult};
use agg_gui::geometry::{Point, Rect, Size};
use agg_gui::text::Font;
use agg_gui::widget::Widget;
use nalgebra::UnitQuaternion;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::view_quat_heading_rad;

/// Horizontal compass strip widget. Derives the user's compass
/// heading from the shared `view_quat` so it stays in lockstep with
/// the sky-view projection (gimbal-lock-free, even at the zenith).
pub struct HorizonTapeWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    view_quat: Rc<Cell<UnitQuaternion<f64>>>,
}

impl HorizonTapeWidget {
    pub fn new(font: Arc<Font>, view_quat: Rc<Cell<UnitQuaternion<f64>>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            view_quat,
        }
    }

    fn fill_rect(ctx: &mut dyn DrawCtx, r: Rect, color: Color) {
        ctx.set_fill_color(color);
        ctx.begin_path();
        ctx.rect(r.x, r.y, r.width, r.height);
        ctx.fill();
    }

    fn stroke_segment(ctx: &mut dyn DrawCtx, a: Point, b: Point, width: f64, color: Color) {
        ctx.set_stroke_color(color);
        ctx.set_line_width(width);
        ctx.begin_path();
        ctx.move_to(a.x, a.y);
        ctx.line_to(b.x, b.y);
        ctx.stroke();
    }
}

const TAPE_HEIGHT: f64 = 28.0;

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

    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, TAPE_HEIGHT);
        Size::new(available.width, TAPE_HEIGHT)
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let b = self.bounds;
        let w = b.width;
        let h = b.height;

        // Background bar.
        Self::fill_rect(ctx, Rect::new(0.0, 0.0, w, h), Color::from_rgba8(20, 20, 30, 200));

        let border = Color::from_rgba8(255, 255, 255, 40);
        Self::stroke_segment(ctx, Point::new(0.0, h), Point::new(w, h), 1.0, border);
        Self::stroke_segment(ctx, Point::new(0.0, 0.0), Point::new(w, 0.0), 1.0, border);

        // Centre indicator (Y-up: the "top" of the bar is `h`).
        let cx = w / 2.0;
        let indicator = Color::from_rgb8(255, 100, 100);
        Self::stroke_segment(ctx, Point::new(cx, h), Point::new(cx, h - 6.0), 2.0, indicator);
        Self::stroke_segment(ctx, Point::new(cx, 0.0), Point::new(cx, 6.0), 2.0, indicator);

        // Compass marks: 4 px per degree, ±half-screen.
        //
        // The shared `view_quat` is the world→view rotation;
        // `view_quat_heading_rad` extracts the camera-forward direction's
        // W3C alpha (CCW from north). Compass tape labels are in the
        // standard CW convention (N=0, E=90, S=180, W=270), so negate
        // and normalise into [0, 360).
        let yaw_w3c_deg = view_quat_heading_rad(self.view_quat.get()).to_degrees();
        let mut yaw_deg = -yaw_w3c_deg;
        yaw_deg = ((yaw_deg % 360.0) + 360.0) % 360.0;
        let pixels_per_degree = 4.0_f64;
        let half_visible_deg = (cx / pixels_per_degree) as i32;
        let start_deg = (yaw_deg as i32 - half_visible_deg - 5).max(-360);
        let end_deg = (yaw_deg as i32 + half_visible_deg + 5).min(720);

        ctx.set_font(Arc::clone(&self.font));

        for deg in start_deg..=end_deg {
            let norm_deg = ((deg % 360) + 360) % 360;
            let deg_diff = deg as f64 - yaw_deg;
            let tick_x = cx + deg_diff * pixels_per_degree;
            if tick_x < 0.0 || tick_x > w {
                continue;
            }

            let is_cardinal = norm_deg % 45 == 0;
            let is_major = norm_deg % 30 == 0;
            let tick_color = Color::from_rgba8(255, 255, 255, 150);

            if is_cardinal {
                Self::stroke_segment(ctx, Point::new(tick_x, 0.0), Point::new(tick_x, 10.0), 1.5, tick_color);
                Self::stroke_segment(ctx, Point::new(tick_x, h), Point::new(tick_x, h - 10.0), 1.5, tick_color);
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
                let color = if norm_deg == 0 {
                    Color::from_rgb8(255, 100, 100)
                } else {
                    Color::from_rgb8(220, 220, 220)
                };
                ctx.set_fill_color(color);
                ctx.set_font_size(11.0);
                let text_w = label.len() as f64 * 7.0;
                ctx.fill_text(label, tick_x - text_w / 2.0, h / 2.0 - 4.0);
            } else if is_major {
                Self::stroke_segment(ctx, Point::new(tick_x, 0.0), Point::new(tick_x, 6.0), 1.0, tick_color);
                Self::stroke_segment(ctx, Point::new(tick_x, h), Point::new(tick_x, h - 6.0), 1.0, tick_color);
                let label = format!("{}", norm_deg);
                ctx.set_fill_color(Color::from_rgba8(255, 255, 255, 100));
                ctx.set_font_size(8.0);
                let text_w = label.len() as f64 * 5.0;
                ctx.fill_text(&label, tick_x - text_w / 2.0, h / 2.0 - 3.0);
            } else if norm_deg % 5 == 0 {
                let minor = Color::from_rgba8(255, 255, 255, 60);
                Self::stroke_segment(ctx, Point::new(tick_x, 0.0), Point::new(tick_x, 4.0), 0.8, minor);
                Self::stroke_segment(ctx, Point::new(tick_x, h), Point::new(tick_x, h - 4.0), 0.8, minor);
            }
        }
    }
}
