//! # Sky View Viewport Widget
//!
//! Full-bleed celestial sphere viewport. All rendering runs through agg-gui's
//! [`DrawCtx`] — no separate wgpu/canvas paths — so the same widget tree
//! works native and WASM. The widget pulls equatorial coordinates from
//! [`crate::stars`], applies the LST → Alt/Az → 3D unit sphere transform from
//! [`crate::math`], multiplies through the device's smoothed orientation
//! matrix, and paints stars / planets / labels as 2-D primitives.
//!
//! Mouse drag inside the viewport rotates the view (yaw + pitch), so the app
//! is testable on desktop where no real device-orientation events arrive.

use crate::math::{
    device_orientation_matrix, equatorial_to_horizontal, horizontal_to_cartesian,
    HorizontalCoords, LowPassFilter,
};
use crate::stars::{calculate_solar_system_bodies, BRIGHTEST_STARS, CONSTELLATION_LINES};

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult, MouseButton};
use agg_gui::geometry::{Point, Rect, Size};
use agg_gui::text::Font;
use agg_gui::widget::Widget;
use std::cell::Cell;
use std::f64::consts::PI;
use std::rc::Rc;
use std::sync::Arc;

/// Sky viewport widget — paints stars, constellations, and Solar System
/// bodies into the current `DrawCtx`.
pub struct SkyViewWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,

    latitude: Rc<Cell<f64>>,
    longitude: Rc<Cell<f64>>,
    timestamp_ms: Rc<Cell<i64>>,

    yaw: Rc<Cell<f64>>,
    pitch: Rc<Cell<f64>>,
    roll: Rc<Cell<f64>>,
    filter: LowPassFilter,

    show_constellations: Rc<Cell<bool>>,

    drag_start: Option<Point>,
}

impl SkyViewWidget {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        font: Arc<Font>,
        latitude: Rc<Cell<f64>>,
        longitude: Rc<Cell<f64>>,
        timestamp_ms: Rc<Cell<i64>>,
        yaw: Rc<Cell<f64>>,
        pitch: Rc<Cell<f64>>,
        roll: Rc<Cell<f64>>,
        show_constellations: Rc<Cell<bool>>,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            latitude,
            longitude,
            timestamp_ms,
            yaw,
            pitch,
            roll,
            // κ = 0.12 (telemetry smoothing modifier) per section 4.1 of implementation.md
            filter: LowPassFilter::new(0.12),
            show_constellations,
            drag_start: None,
        }
    }

    /// Project a horizontal-frame coordinate through the device orientation
    /// matrix and perspective camera. Returns `None` if the point is behind
    /// the virtual camera (so we don't paint stars on the back of the
    /// observer's head).
    fn project_horizontal(
        &self,
        coords: HorizontalCoords,
        rot_matrix: &nalgebra::Matrix3<f64>,
        center: Point,
        focal_length: f64,
    ) -> Option<Point> {
        let v_cart = horizontal_to_cartesian(coords);
        let v_rot = rot_matrix * v_cart;
        let (x, y, z) = (v_rot.x, v_rot.y, v_rot.z);
        if z <= 0.05 {
            return None;
        }
        Some(Point::new(
            center.x + (x / z) * focal_length,
            center.y + (y / z) * focal_length,
        ))
    }

    fn fill_rect(ctx: &mut dyn DrawCtx, r: Rect, color: Color) {
        ctx.set_fill_color(color);
        ctx.begin_path();
        ctx.rect(r.x, r.y, r.width, r.height);
        ctx.fill();
    }

    fn fill_disc(ctx: &mut dyn DrawCtx, p: Point, radius: f64, color: Color) {
        ctx.set_fill_color(color);
        ctx.begin_path();
        ctx.circle(p.x, p.y, radius);
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

    fn draw_text(ctx: &mut dyn DrawCtx, p: Point, size: f64, color: Color, text: &str) {
        ctx.set_fill_color(color);
        ctx.set_font_size(size);
        ctx.fill_text(text, p.x, p.y);
    }
}

impl Widget for SkyViewWidget {
    fn type_name(&self) -> &'static str {
        "SkyViewWidget"
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
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn hit_test(&self, _local_pos: Point) -> bool {
        true
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { pos, button: MouseButton::Left, .. } => {
                self.drag_start = Some(*pos);
                EventResult::Consumed
            }
            Event::MouseMove { pos } => {
                if let Some(start) = self.drag_start {
                    let dx = pos.x - start.x;
                    let dy = pos.y - start.y;
                    let sensitivity = 0.003;

                    let mut new_yaw = self.yaw.get() - dx * sensitivity;
                    while new_yaw < 0.0 {
                        new_yaw += 2.0 * PI;
                    }
                    while new_yaw >= 2.0 * PI {
                        new_yaw -= 2.0 * PI;
                    }

                    let new_pitch = (self.pitch.get() + dy * sensitivity)
                        .clamp(-PI / 2.0 + 0.01, PI / 2.0 - 0.01);

                    self.yaw.set(new_yaw);
                    self.pitch.set(new_pitch);

                    self.drag_start = Some(*pos);
                    agg_gui::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseUp { button: MouseButton::Left, .. } => {
                self.drag_start = None;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let b = self.bounds;
        let w = b.width;
        let h = b.height;

        // Night-sky backdrop (deep indigo).
        Self::fill_rect(ctx, Rect::new(0.0, 0.0, w, h), Color::from_rgb8(10, 10, 25));

        let center = Point::new(w / 2.0, h * 0.6);
        let focal_length = (w.min(h)) * 0.9;

        let (smooth_yaw, smooth_pitch, smooth_roll) = self.filter.update(
            self.yaw.get(),
            self.pitch.get(),
            self.roll.get(),
        );
        let rot = device_orientation_matrix(smooth_yaw, smooth_pitch, smooth_roll);

        // State cells hold latitude / longitude in **degrees** (user-facing
        // units, matching the city DB and what the status readout displays);
        // convert once to radians here for the projection pipeline.
        let lst = crate::math::compute_local_sidereal_time(
            self.timestamp_ms.get(),
            self.longitude.get().to_radians(),
        );
        let lat = self.latitude.get().to_radians();

        // Constellation lines (optional).
        if self.show_constellations.get() {
            let line_color = Color::from_rgba8(100, 150, 255, 100);
            for line in CONSTELLATION_LINES {
                let from = BRIGHTEST_STARS.iter().find(|s| s.id == line.from_id);
                let to = BRIGHTEST_STARS.iter().find(|s| s.id == line.to_id);
                if let (Some(from), Some(to)) = (from, to) {
                    let h_from = equatorial_to_horizontal(from.coords, lat, lst);
                    let h_to = equatorial_to_horizontal(to.coords, lat, lst);
                    if let (Some(p_from), Some(p_to)) = (
                        self.project_horizontal(h_from, &rot, center, focal_length),
                        self.project_horizontal(h_to, &rot, center, focal_length),
                    ) {
                        Self::stroke_segment(ctx, p_from, p_to, 1.0, line_color);
                    }
                }
            }
        }

        // Stars.
        ctx.set_font(Arc::clone(&self.font));
        for star in BRIGHTEST_STARS {
            let horiz = equatorial_to_horizontal(star.coords, lat, lst);
            let Some(pos) = self.project_horizontal(horiz, &rot, center, focal_length) else {
                continue;
            };
            if pos.x < 0.0 || pos.x > w || pos.y < 0.0 || pos.y > h {
                continue;
            }
            let mag = star.magnitude as f64;
            let radius = (3.5 - mag).clamp(1.0, 6.0);
            let color = if star.color_index < 0.2 {
                Color::from_rgb8(180, 210, 255)
            } else if star.color_index > 1.0 {
                Color::from_rgb8(255, 180, 130)
            } else {
                Color::from_rgb8(255, 255, 255)
            };
            Self::fill_disc(ctx, pos, radius, color);

            if star.magnitude < 1.0 {
                Self::draw_text(
                    ctx,
                    Point::new(pos.x + radius + 3.0, pos.y - 3.0),
                    9.0,
                    Color::from_rgba8(220, 220, 255, 180),
                    star.name,
                );
            }
        }

        // Solar System bodies.
        for body in calculate_solar_system_bodies(self.timestamp_ms.get()) {
            let horiz = equatorial_to_horizontal(body.coords, lat, lst);
            let Some(pos) = self.project_horizontal(horiz, &rot, center, focal_length) else {
                continue;
            };
            if pos.x < -20.0 || pos.x > w + 20.0 || pos.y < -20.0 || pos.y > h + 20.0 {
                continue;
            }
            let radius = match body.name {
                "Sun" => 16.0,
                "Moon" => 12.0,
                _ => 5.0,
            };
            if body.name == "Sun" {
                Self::fill_disc(ctx, pos, radius + 4.0, Color::from_rgba8(255, 200, 50, 60));
            }
            Self::fill_disc(ctx, pos, radius, body.color);
            Self::draw_text(
                ctx,
                Point::new(pos.x + radius + 4.0, pos.y - 4.0),
                11.0,
                Color::from_rgb8(255, 255, 255),
                body.name,
            );
        }

        // Horizon ring — paints cardinal directions at altitude 0 so the
        // user can orient themselves before the device telemetry kicks in.
        let directions: [(&str, f64); 8] = [
            ("N", 0.0),
            ("NE", PI / 4.0),
            ("E", PI / 2.0),
            ("SE", 3.0 * PI / 4.0),
            ("S", PI),
            ("SW", 5.0 * PI / 4.0),
            ("W", 3.0 * PI / 2.0),
            ("NW", 7.0 * PI / 4.0),
        ];
        let horizon = Color::from_rgba8(255, 100, 100, 120);
        for (name, az) in directions {
            let hc = HorizontalCoords { alt: 0.0, az };
            if let Some(pos) = self.project_horizontal(hc, &rot, center, focal_length) {
                if pos.x >= 0.0 && pos.x <= w && pos.y >= 0.0 && pos.y <= h {
                    Self::fill_disc(ctx, pos, 3.0, horizon);
                    Self::draw_text(
                        ctx,
                        Point::new(pos.x - 6.0, pos.y + 6.0),
                        12.0,
                        horizon,
                        name,
                    );
                }
            }
        }
    }
}
