//! # Sky View 3D/2D Viewport Widget
//!
//! This widget implements the interactive 3D celestial sphere projection.
//! It transforms equatorial coordinates of stars, constellations, and Solar System
//! bodies into local horizontal coordinates (Alt/Az), applies the device's smoothed
//! orientation matrix, and projects them with a realistic perspective camera onto
//! the 2D widget viewport.

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

/// Widget rendering the full-bleed celestial sky sphere.
pub struct SkyViewWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,

    // Application state links
    latitude: Rc<Cell<f64>>,
    longitude: Rc<Cell<f64>>,
    timestamp_ms: Rc<Cell<i64>>,

    // Telemetry and filter state
    yaw: Rc<Cell<f64>>,
    pitch: Rc<Cell<f64>>,
    roll: Rc<Cell<f64>>,
    filter: LowPassFilter,

    // Toggles
    show_constellations: Rc<Cell<bool>>,

    // Interactive mouse drag tracking
    drag_start: Option<Point>,
}

impl SkyViewWidget {
    /// Create a new interactive sky viewport widget.
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
            filter: LowPassFilter::new(0.12), // Smoothing modifier (kappa = 0.12)
            show_constellations,
            drag_start: None,
        }
    }

    /// Project a horizontal celestial coordinate onto the 2D screen coordinate.
    /// Returns `Some((x, y))` on success, or `None` if the point is out of range or behind the camera.
    fn project_horizontal(
        &mut self,
        coords: HorizontalCoords,
        rot_matrix: &nalgebra::Matrix3<f64>,
        center: Point,
        focal_length: f64,
    ) -> Option<Point> {
        // 1. Convert horizontal coordinates to 3D unit vector
        let v_cart = horizontal_to_cartesian(coords);

        // 2. Rotate unit vector based on device orientation matrix
        let v_rot = rot_matrix * v_cart;

        // In camera coordinates, say:
        // Z' is depth (positive Z' is forward/in-front)
        // X' is right
        // Y' is up
        let x = v_rot.x;
        let y = v_rot.y;
        let z = v_rot.z;

        // Clip things behind the camera (z <= 0)
        if z <= 0.05 {
            return None;
        }

        // 3. Perspective Projection
        let proj_x = center.x + (x / z) * focal_length;
        let proj_y = center.y + (y / z) * focal_length;

        Some(Point::new(proj_x, proj_y))
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
        true // Entire viewport of the sky is interactive for mouse panning
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

                    // Convert drag distance to angular rotation in radians.
                    // Yaw (azimuth) wraps from 0 to 2*PI.
                    // Pitch (altitude) is clamped to avoid flipping upside down.
                    let sensitivity = 0.003;
                    let mut new_yaw = self.yaw.get() - dx * sensitivity;
                    while new_yaw < 0.0 {
                        new_yaw += 2.0 * PI;
                    }
                    while new_yaw >= 2.0 * PI {
                        new_yaw -= 2.0 * PI;
                    }

                    let new_pitch = (self.pitch.get() + dy * sensitivity).clamp(-PI / 2.0 + 0.01, PI / 2.0 - 0.01);

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

        // 1. Draw Space Background (Deep Indigo gradient or solid black)
        let sky_color = Color::from_rgb8(10, 10, 25);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.set_fill_color(sky_color);
        ctx.fill();

        // Calculate center and focal length (FOV control)
        let center = Point::new(w / 2.0, h * 0.6);
        let focal_length = (w.min(h) as f64) * 0.9;

        // 2. Apply Telemetry Smoothing
        let (smooth_yaw, smooth_pitch, smooth_roll) = self.filter.update(
            self.yaw.get(),
            self.pitch.get(),
            self.roll.get()
        );

        // Construct 3D rotation matrix
        let rot_matrix = device_orientation_matrix(smooth_yaw, smooth_pitch, smooth_roll);

        // Fetch current Sidereal Time
        let lst = crate::math::compute_local_sidereal_time(
            self.timestamp_ms.get(),
            self.longitude.get()
        );

        let lat = self.latitude.get();

        // 3. Project and Render Constellation Lines (if enabled)
        if self.show_constellations.get() {
            let line_color = Color::from_rgba8(100, 150, 255, 100); // Soft glowing blue
            for line in CONSTELLATION_LINES {
                // Find start and end stars
                let star_from = BRIGHTEST_STARS.iter().find(|s| s.id == line.from_id);
                let star_to = BRIGHTEST_STARS.iter().find(|s| s.id == line.to_id);

                if let (Some(from), Some(to)) = (star_from, star_to) {
                    let h_from = equatorial_to_horizontal(from.coords, lat, lst);
                    let h_to = equatorial_to_horizontal(to.coords, lat, lst);

                    let p_from = self.project_horizontal(h_from, &rot_matrix, center, focal_length);
                    let p_to = self.project_horizontal(h_to, &rot_matrix, center, focal_length);

                    if let (Some(pf), Some(pt)) = (p_from, p_to) {
                        ctx.begin_path();
                        ctx.move_to(pf.x, pf.y);
                        ctx.line_to(pt.x, pt.y);
                        ctx.set_stroke_color(line_color);
                        ctx.set_line_width(1.0);
                        ctx.stroke();
                    }
                }
            }
        }

        // 4. Project and Render Stars
        for star in BRIGHTEST_STARS {
            let horiz = equatorial_to_horizontal(star.coords, lat, lst);
            if let Some(pos) = self.project_horizontal(horiz, &rot_matrix, center, focal_length) {
                // Ignore rendering if star lands outside the viewport
                if pos.x >= 0.0 && pos.x <= w && pos.y >= 0.0 && pos.y <= h {
                    // Radius based on visual magnitude (brighter stars are larger)
                    let mag_val = star.magnitude as f64;
                    let radius = (3.5 - mag_val).max(1.0).min(6.0);

                    // Color based on B-V color index
                    let star_color = if star.color_index < 0.2 {
                        Color::from_rgb8(180, 210, 255) // Blue-ish
                    } else if star.color_index > 1.0 {
                        Color::from_rgb8(255, 180, 130) // Red-ish/Orange-ish
                    } else {
                        Color::from_rgb8(255, 255, 255) // White
                    };

                    ctx.begin_path();
                    ctx.circle(pos.x, pos.y, radius);
                    ctx.set_fill_color(star_color);
                    ctx.fill();

                    // If it is a famous bright star, render its label
                    if star.magnitude < 1.0 {
                        ctx.set_font(Arc::clone(&self.font));
                        ctx.set_font_size(9.0);
                        let label_color = Color::from_rgba8(220, 220, 255, 180);
                        ctx.set_fill_color(label_color);
                        ctx.fill_text(
                            &star.name,
                            pos.x + radius + 3.0,
                            pos.y - 3.0
                        );
                    }
                }
            }
        }

        // 5. Project and Render Solar System Bodies (Sun, Moon, Planets)
        let planetary_bodies = calculate_solar_system_bodies(self.timestamp_ms.get());
        for body in planetary_bodies {
            let horiz = equatorial_to_horizontal(body.coords, lat, lst);
            if let Some(pos) = self.project_horizontal(horiz, &rot_matrix, center, focal_length) {
                if pos.x >= -20.0 && pos.x <= w + 20.0 && pos.y >= -20.0 && pos.y <= h + 20.0 {
                    let radius = if body.name == "Sun" {
                        16.0
                    } else if body.name == "Moon" {
                        12.0
                    } else {
                        5.0
                    };

                    // Draw glowing aura for Sun
                    if body.name == "Sun" {
                        ctx.begin_path();
                        ctx.circle(pos.x, pos.y, radius + 4.0);
                        ctx.set_fill_color(Color::from_rgba8(255, 200, 50, 60));
                        ctx.fill();
                    }

                    ctx.begin_path();
                    ctx.circle(pos.x, pos.y, radius);
                    ctx.set_fill_color(body.color);
                    ctx.fill();

                    // Write body name label
                    ctx.set_font(Arc::clone(&self.font));
                    ctx.set_font_size(11.0);
                    ctx.set_fill_color(Color::from_rgb8(255, 255, 255));
                    ctx.fill_text(
                        body.name,
                        pos.x + radius + 4.0,
                        pos.y - 4.0
                    );
                }
            }
        }

        // 6. Draw Horizon Overlay (if the camera is looking flat or down)
        // Let's draw a beautiful Compass rose or simple Cardinal indicators at alt=0
        let directions = [
            ("N", 0.0),
            ("NE", PI / 4.0),
            ("E", PI / 2.0),
            ("SE", 3.0 * PI / 4.0),
            ("S", PI),
            ("SW", 5.0 * PI / 4.0),
            ("W", 3.0 * PI / 2.0),
            ("NW", 7.0 * PI / 4.0),
        ];

        let horizon_color = Color::from_rgba8(255, 100, 100, 120);
        for (name, az) in &directions {
            let h_coord = HorizontalCoords { alt: 0.0, az: *az };
            if let Some(pos) = self.project_horizontal(h_coord, &rot_matrix, center, focal_length) {
                if pos.x >= 0.0 && pos.x <= w && pos.y >= 0.0 && pos.y <= h {
                    ctx.begin_path();
                    ctx.circle(pos.x, pos.y, 3.0);
                    ctx.set_fill_color(horizon_color);
                    ctx.fill();

                    ctx.set_font(Arc::clone(&self.font));
                    ctx.set_font_size(12.0);
                    ctx.set_fill_color(horizon_color);
                    ctx.fill_text(
                        name,
                        pos.x - 6.0,
                        pos.y + 6.0
                    );
                }
            }
        }
    }
}
