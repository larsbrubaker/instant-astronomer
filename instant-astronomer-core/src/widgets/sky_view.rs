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
//! A short tap (no drag) identifies the celestial body nearest the click and
//! pins an info card on it — the core "what's that bright thing on the
//! horizon?" lookup the app was built for.

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
use std::cell::{Cell, RefCell};
use std::f64::consts::PI;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

/// Maximum distance (logical pixels) and dwell time the pointer can move
/// between MouseDown and MouseUp for the gesture to count as a tap. Beyond
/// these the gesture is treated as a pan / drag.
const TAP_MAX_DRIFT: f64 = 4.0;
const TAP_MAX_DURATION_MS: u128 = 350;
/// Maximum distance from the tap position to a celestial body before the
/// hit is rejected. Generous so finger taps on a 320 px wide phone land.
const TAP_HIT_RADIUS: f64 = 28.0;

/// A celestial body that was painted in the previous frame, together with
/// the screen position where it landed. Cached so the tap-to-identify hit
/// test can run in O(n) against actual on-screen geometry instead of
/// re-running the full projection pipeline.
#[derive(Debug, Clone)]
struct PaintedBody {
    name: String,
    pos: Point,
    /// Apparent visual magnitude. Smaller = brighter; planets / bright
    /// stars get priority when two bodies sit close together.
    magnitude: f32,
    /// Optional extra description shown in the info card.
    detail: Option<String>,
}

/// Information about the currently selected (tapped) body, painted as an
/// info card on top of the sky.
#[derive(Debug, Clone)]
struct Selection {
    name: String,
    magnitude: f32,
    detail: Option<String>,
}

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

    /// Set on MouseDown, cleared on MouseUp / MouseLeave. While set we
    /// track whether the pointer drifted enough to count as a drag.
    down: Option<DownGesture>,
    /// Latest cache of celestial bodies projected in the previous paint —
    /// the input to tap hit-testing.
    painted_bodies: RefCell<Vec<PaintedBody>>,
    /// Body the user most recently tapped on. Renders as an info card.
    selected: Option<Selection>,
}

#[derive(Debug, Clone, Copy)]
struct DownGesture {
    /// Where the pointer touched down (widget-local Y-up coordinates).
    origin: Point,
    /// Last pointer position observed during the gesture.
    last: Point,
    started_at: Instant,
    is_drag: bool,
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
            down: None,
            painted_bodies: RefCell::new(Vec::new()),
            selected: None,
        }
    }

    /// Run a tap hit test against the cached painted bodies. Picks the
    /// closest hit within [`TAP_HIT_RADIUS`]; on ties (e.g. an overlapping
    /// planet + bright star), prefer the brighter body so taps on Venus
    /// don't get swallowed by a fainter background star.
    fn hit_test_tap(&self, tap_pos: Point) -> Option<PaintedBody> {
        let bodies = self.painted_bodies.borrow();
        let mut best: Option<(f64, PaintedBody)> = None;
        for body in bodies.iter() {
            let dx = body.pos.x - tap_pos.x;
            let dy = body.pos.y - tap_pos.y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > TAP_HIT_RADIUS {
                continue;
            }
            // Score: distance + magnitude scaled, so a slightly farther but
            // visibly brighter body wins over a faint nearby star.
            let score = dist + (body.magnitude as f64) * 4.0;
            match &best {
                Some((best_score, _)) if score >= *best_score => {}
                _ => best = Some((score, body.clone())),
            }
        }
        best.map(|(_, b)| b)
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
                self.down = Some(DownGesture {
                    origin: *pos,
                    last: *pos,
                    started_at: Instant::now(),
                    is_drag: false,
                });
                EventResult::Consumed
            }
            Event::MouseMove { pos } => {
                let Some(down) = self.down.as_mut() else {
                    return EventResult::Ignored;
                };
                let dx_total = pos.x - down.origin.x;
                let dy_total = pos.y - down.origin.y;
                if !down.is_drag
                    && (dx_total * dx_total + dy_total * dy_total).sqrt() > TAP_MAX_DRIFT
                {
                    down.is_drag = true;
                }
                if down.is_drag {
                    let dx = pos.x - down.last.x;
                    let dy = pos.y - down.last.y;
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
                    agg_gui::animation::request_draw();
                }
                down.last = *pos;
                EventResult::Consumed
            }
            Event::MouseUp { pos, button: MouseButton::Left, .. } => {
                let Some(down) = self.down.take() else {
                    return EventResult::Ignored;
                };
                let elapsed = down.started_at.elapsed();
                let is_tap = !down.is_drag && elapsed < Duration::from_millis(TAP_MAX_DURATION_MS as u64);
                if is_tap {
                    if let Some(hit) = self.hit_test_tap(*pos) {
                        self.selected = Some(Selection {
                            name: hit.name,
                            magnitude: hit.magnitude,
                            detail: hit.detail,
                        });
                    } else {
                        self.selected = None;
                    }
                    agg_gui::animation::request_draw();
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let b = self.bounds;
        let w = b.width;
        let h = b.height;

        // Reset the painted-bodies cache for this frame; will be filled in
        // as we project stars / planets.
        let mut painted: Vec<PaintedBody> = Vec::new();

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

        // Stars. Cull anything below the real-world horizon so the
        // ground band at the bottom of the screen reads as actual
        // ground — no stars peeking out from below.
        ctx.set_font(Arc::clone(&self.font));
        for star in BRIGHTEST_STARS {
            let horiz = equatorial_to_horizontal(star.coords, lat, lst);
            if horiz.alt < 0.0 {
                continue;
            }
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

            painted.push(PaintedBody {
                name: star.name.to_string(),
                pos,
                magnitude: star.magnitude,
                detail: Some(format!(
                    "Star · mag {:.1} · RA {:.2}h · Dec {:+.1}°",
                    star.magnitude,
                    star.coords.ra.to_degrees() / 15.0,
                    star.coords.dec.to_degrees(),
                )),
            });
        }

        // Solar System bodies. Render brighter / larger discs for the body
        // sizes the user cares about (Sun, Moon big; Venus + Jupiter
        // notably brighter than fixed stars; the others sit between).
        // Same below-horizon cull as for stars — the Sun and planets
        // can plausibly be below the horizon, and we don't want them
        // floating above the ground band.
        for body in calculate_solar_system_bodies(self.timestamp_ms.get()) {
            let horiz = equatorial_to_horizontal(body.coords, lat, lst);
            if horiz.alt < 0.0 {
                continue;
            }
            let Some(pos) = self.project_horizontal(horiz, &rot, center, focal_length) else {
                continue;
            };
            if pos.x < -20.0 || pos.x > w + 20.0 || pos.y < -20.0 || pos.y > h + 20.0 {
                continue;
            }
            // Disc size: scale roughly by visual magnitude — Sun/Moon get
            // fixed-large radii; planets scale by brightness.
            let radius = match body.name {
                "Sun" => 18.0,
                "Moon" => 14.0,
                "Venus" => 7.0,
                "Jupiter" => 6.5,
                "Mars" | "Saturn" => 5.5,
                _ => 5.0,
            };
            // Sun and Moon get a soft glow halo.
            if body.name == "Sun" {
                Self::fill_disc(ctx, pos, radius + 6.0, Color::from_rgba8(255, 200, 50, 60));
            } else if body.name == "Moon" {
                Self::fill_disc(ctx, pos, radius + 3.0, Color::from_rgba8(220, 220, 240, 50));
            } else if body.name == "Venus" || body.name == "Jupiter" {
                // The two "evening star" objects deserve their own glow so
                // they read at a glance — the entire reason this app exists.
                Self::fill_disc(ctx, pos, radius + 3.0, Color::from_rgba8(255, 240, 200, 60));
            }
            Self::fill_disc(ctx, pos, radius, body.color);
            Self::draw_text(
                ctx,
                Point::new(pos.x + radius + 4.0, pos.y - 4.0),
                12.0,
                Color::from_rgb8(255, 255, 255),
                body.name,
            );

            let detail = if body.name == "Sun" || body.name == "Moon" {
                Some(format!("Solar System · mag {:.1}", body.magnitude))
            } else {
                Some(format!(
                    "Planet · mag {:.1} · alt {:+.0}° · az {:.0}°",
                    body.magnitude,
                    horiz.alt.to_degrees(),
                    horiz.az.to_degrees(),
                ))
            };
            painted.push(PaintedBody {
                name: body.name.to_string(),
                pos,
                magnitude: body.magnitude,
                detail,
            });
        }

        // Horizon strip — a stable horizontal reference at the bottom
        // of the viewport so the user always knows where the ground is,
        // no matter how they pan / tilt the phone. Cardinal direction
        // labels (N / NE / E / …) slide along the strip based on the
        // user's current heading, matching the actual real-world
        // direction each label points at on the celestial sphere.
        Self::paint_horizon_strip(ctx, Arc::clone(&self.font), w, h, &rot, center, focal_length);

        // Selected-body info card. Drawn last so the panel sits above any
        // overlapping stars / labels. We look the selection up in the
        // freshly-painted set so the card moves with the body as the user
        // pans, and disappears automatically if the body slid off-screen.
        if let Some(sel) = self.selected.clone() {
            if let Some(body) = painted.iter().find(|p| p.name == sel.name).cloned() {
                Self::paint_info_card(
                    ctx,
                    Arc::clone(&self.font),
                    body.pos,
                    Rect::new(0.0, 0.0, w, h),
                    &sel,
                );
            }
        }

        // Promote this frame's projections to the cache for the next tap.
        self.painted_bodies.replace(painted);
    }
}

impl SkyViewWidget {
    /// Paint a horizontal horizon line at the bottom of the sky view
    /// with a faint "ground" band below it and cardinal direction
    /// labels (N / NE / E / …) sliding along its top edge.
    ///
    /// The line itself sits at a fixed Y so the user always has a
    /// stable bottom-of-screen reference — a screen orientation
    /// you can trust no matter how the phone is rotated. Cardinal
    /// labels use the *current* projection of each compass direction
    /// on the celestial sphere to pick their X position, so as the
    /// user pans the sky the labels slide accordingly.
    fn paint_horizon_strip(
        ctx: &mut dyn DrawCtx,
        font: Arc<Font>,
        w: f64,
        _h: f64,
        rot: &nalgebra::Matrix3<f64>,
        center: Point,
        focal_length: f64,
    ) {
        // Bottom band reserved for the ground + horizon line. Tuned so
        // it doesn't eat too much sky on small phones but is still big
        // enough for a readable label row.
        let ground_h = 36.0_f64;
        let horizon_y = ground_h; // top edge of the ground band (Y-up)

        // Ground fill: subtle dark band so the eye knows "this isn't sky".
        ctx.set_fill_color(Color::from_rgba8(4, 4, 10, 220));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, ground_h);
        ctx.fill();

        // Soft horizon glow just above the line — lifts the line off
        // the deep-indigo sky so it reads as the horizon, not just a
        // UI divider.
        for i in 0..6 {
            let alpha = 18 - i * 3;
            let yy = horizon_y + i as f64;
            ctx.set_stroke_color(Color::from_rgba8(120, 100, 80, alpha.max(0) as u8));
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.move_to(0.0, yy);
            ctx.line_to(w, yy);
            ctx.stroke();
        }

        // The horizon line itself — warm tone, clearly visible against
        // the indigo sky.
        ctx.set_stroke_color(Color::from_rgba8(255, 180, 120, 200));
        ctx.set_line_width(1.2);
        ctx.begin_path();
        ctx.move_to(0.0, horizon_y);
        ctx.line_to(w, horizon_y);
        ctx.stroke();

        // Cardinal labels: compute the projected X for each direction
        // on the alt=0 ring and pin the label at that X with Y = on
        // the horizon line. Skip labels whose projected direction is
        // behind the camera (depth <= 0).
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

        ctx.set_font(font);
        for (name, az) in directions {
            let hc = HorizontalCoords { alt: 0.0, az };
            let v_cart = horizontal_to_cartesian(hc);
            let v_rot = rot * v_cart;
            let (x, _, z) = (v_rot.x, v_rot.y, v_rot.z);
            if z <= 0.05 {
                continue;
            }
            let projected_x = center.x + (x / z) * focal_length;
            if projected_x < -20.0 || projected_x > w + 20.0 {
                continue;
            }

            // Tick mark straddling the horizon line.
            ctx.set_stroke_color(Color::from_rgba8(255, 200, 140, 220));
            ctx.set_line_width(if name.len() == 1 { 1.6 } else { 1.0 });
            ctx.begin_path();
            ctx.move_to(projected_x, horizon_y - 6.0);
            ctx.line_to(projected_x, horizon_y + 6.0);
            ctx.stroke();

            // Cardinal label below the line. Bigger / brighter for the
            // four cardinals (N, E, S, W); smaller / dimmer for the
            // inter-cardinals.
            let is_cardinal = name.len() == 1;
            let label_size = if is_cardinal { 13.0 } else { 10.0 };
            let label_color = if is_cardinal {
                if name == "N" {
                    Color::from_rgb8(255, 110, 110) // red for North
                } else {
                    Color::from_rgb8(255, 220, 160)
                }
            } else {
                Color::from_rgba8(255, 200, 150, 180)
            };
            let approx_w = name.chars().count() as f64 * label_size * 0.6;
            ctx.set_fill_color(label_color);
            ctx.set_font_size(label_size);
            ctx.fill_text(name, projected_x - approx_w / 2.0, 6.0);
        }
    }

    /// Paint a small info card anchored near `target`. Card stays inside the
    /// `viewport` rect — flips to the other side of the body if it would
    /// otherwise clip the right / top edges.
    fn paint_info_card(
        ctx: &mut dyn DrawCtx,
        font: Arc<Font>,
        target: Point,
        viewport: Rect,
        sel: &Selection,
    ) {
        let mut lines: Vec<String> = Vec::with_capacity(3);
        lines.push(sel.name.clone());
        lines.push(format!("magnitude {:+.2}", sel.magnitude));
        if let Some(detail) = &sel.detail {
            lines.push(detail.clone());
        }

        let title_size = 14.0_f64;
        let body_size = 11.0_f64;
        let pad = 10.0_f64;
        let line_gap = 4.0_f64;

        // Approximate widths from glyph counts — agg-gui's `measure_text`
        // needs a font to be set first; we keep it cheap and consistent.
        let approx_width = |text: &str, size: f64| (text.chars().count() as f64) * size * 0.55;
        let mut card_w = lines
            .iter()
            .enumerate()
            .map(|(i, l)| approx_width(l, if i == 0 { title_size } else { body_size }))
            .fold(0.0_f64, f64::max)
            + pad * 2.0;
        card_w = card_w.clamp(160.0, viewport.width - 24.0);
        let card_h = title_size
            + (lines.len() - 1) as f64 * (body_size + line_gap)
            + line_gap
            + pad * 2.0;

        // Anchor card to the upper-right of the tapped body by default.
        let anchor_dx = 14.0_f64;
        let anchor_dy = 14.0_f64;
        let mut x = target.x + anchor_dx;
        let mut y = target.y + anchor_dy;
        if x + card_w > viewport.width - 8.0 {
            x = target.x - card_w - anchor_dx;
        }
        if y + card_h > viewport.height - 8.0 {
            y = target.y - card_h - anchor_dy;
        }
        x = x.clamp(8.0, viewport.width - card_w - 8.0);
        y = y.clamp(8.0, viewport.height - card_h - 8.0);

        // Backdrop + border.
        ctx.set_fill_color(Color::from_rgba8(15, 20, 38, 230));
        ctx.begin_path();
        ctx.rounded_rect(x, y, card_w, card_h, 8.0);
        ctx.fill();
        ctx.set_stroke_color(Color::from_rgba8(255, 215, 90, 200));
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.rounded_rect(x, y, card_w, card_h, 8.0);
        ctx.stroke();

        // Pointer line from card to the tapped body.
        ctx.set_stroke_color(Color::from_rgba8(255, 215, 90, 180));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(target.x, target.y);
        // Snap pointer to nearest card edge midpoint.
        let cx = x + card_w / 2.0;
        let cy = y + card_h / 2.0;
        let edge_x = if target.x < x {
            x
        } else if target.x > x + card_w {
            x + card_w
        } else {
            cx
        };
        let edge_y = if target.y < y {
            y
        } else if target.y > y + card_h {
            y + card_h
        } else {
            cy
        };
        ctx.line_to(edge_x, edge_y);
        ctx.stroke();

        // Text. Y-up: top of card has the higher y; lines are stacked
        // downward → decreasing y. fill_text places the baseline so add a
        // small offset above the baseline for visual centering.
        ctx.set_font(font);
        let title_baseline = y + card_h - pad - title_size * 0.85;
        ctx.set_fill_color(Color::from_rgb8(255, 235, 150));
        ctx.set_font_size(title_size);
        ctx.fill_text(&lines[0], x + pad, title_baseline);

        ctx.set_fill_color(Color::from_rgb8(220, 222, 240));
        ctx.set_font_size(body_size);
        for (i, line) in lines.iter().enumerate().skip(1) {
            let baseline = title_baseline
                - title_size * 0.15
                - line_gap
                - i as f64 * (body_size + line_gap);
            ctx.fill_text(line, x + pad, baseline);
        }
    }
}
