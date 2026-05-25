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

mod hud;

use crate::math::{
    equatorial_to_horizontal, horizontal_to_cartesian, HorizontalCoords,
};
use crate::stars::{
    all_stars, calculate_solar_system_bodies, zodiac_date_range, CONSTELLATION_LINES,
};
use nalgebra::{UnitQuaternion, Vector3};

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult, MouseButton};
use agg_gui::geometry::{Point, Rect, Size};
use agg_gui::text::Font;
use agg_gui::widget::Widget;
use std::cell::{Cell, RefCell};
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
pub(crate) struct PaintedBody {
    pub name: String,
    pub pos: Point,
    /// Apparent visual magnitude. Smaller = brighter; planets / bright
    /// stars get priority when two bodies sit close together.
    pub magnitude: f32,
    /// Optional extra description shown in the info card.
    pub detail: Option<String>,
}

/// Information about the currently selected (tapped) body, painted as an
/// info card on top of the sky.
#[derive(Debug, Clone)]
pub(crate) struct Selection {
    pub name: String,
    pub magnitude: f32,
    pub detail: Option<String>,
    /// Last-known screen position. Used as a fallback for things that
    /// don't appear in the per-frame `painted` cache (constellation
    /// lines) and as the anchor for the hover card while the cursor
    /// is over a segment.
    pub pos: Point,
}

/// A constellation line segment in screen coordinates after projection.
/// Cached each frame so a tap that misses every body can still resolve
/// to "you tapped the Cygnus spine" by checking distance to nearby
/// segments.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PaintedSegment {
    pub constellation_name: &'static str,
    pub p0: Point,
    pub p1: Point,
}

/// Tap radius for hitting a constellation line. Tighter than
/// [`TAP_HIT_RADIUS`] so a body close to a line still wins; a tap
/// that misses every body but is on the line itself still resolves.
const LINE_HIT_RADIUS: f64 = 12.0;

/// Sky viewport widget — paints stars, constellations, and Solar System
/// bodies into the current `DrawCtx`.
pub struct SkyViewWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,

    latitude: Rc<Cell<f64>>,
    longitude: Rc<Cell<f64>>,
    timestamp_ms: Rc<Cell<i64>>,

    /// World→view rotation. Mouse drag composes camera-local
    /// rotations into this cell; the WASM shell `set()`s it on every
    /// `deviceorientation` event after converting Euler → quaternion.
    /// Using a quaternion sidesteps the gimbal-lock singularity that
    /// the previous Tait-Bryan storage hit at the zenith / nadir.
    view_quat: Rc<Cell<UnitQuaternion<f64>>>,
    /// Compass-offset calibration around the world up axis. Subtracted
    /// before the projection. See the Calibrate button.
    calibration_yaw: Rc<Cell<f64>>,

    show_constellations: Rc<Cell<bool>>,

    /// Set on MouseDown, cleared on MouseUp / MouseLeave. While set we
    /// track whether the pointer drifted enough to count as a drag.
    down: Option<DownGesture>,
    /// Latest cache of celestial bodies projected in the previous paint —
    /// the input to tap hit-testing.
    painted_bodies: RefCell<Vec<PaintedBody>>,
    /// Latest cache of projected constellation line segments. Consulted
    /// by tap hit-testing after `painted_bodies` fails so the user can
    /// tap a constellation line itself to see its name + zodiac date
    /// range (when applicable).
    painted_lines: RefCell<Vec<PaintedSegment>>,
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
        view_quat: Rc<Cell<UnitQuaternion<f64>>>,
        calibration_yaw: Rc<Cell<f64>>,
        show_constellations: Rc<Cell<bool>>,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            latitude,
            longitude,
            timestamp_ms,
            view_quat,
            calibration_yaw,
            show_constellations,
            down: None,
            painted_bodies: RefCell::new(Vec::new()),
            painted_lines: RefCell::new(Vec::new()),
            selected: None,
        }
    }

    /// Run a tap hit test against the cached painted bodies. Picks the
    /// closest hit within [`TAP_HIT_RADIUS`]; on ties (e.g. an overlapping
    /// planet + bright star), prefer the brighter body so taps on Venus
    /// don't get swallowed by a fainter background star.
    ///
    /// If no body is within reach we fall through to a second pass that
    /// hit-tests constellation line segments — taps on the empty space
    /// between two stars in a constellation should still resolve to
    /// "this is Cygnus" (with the zodiac date range for the 12
    /// tropical signs).
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
        if let Some((_, b)) = best {
            return Some(b);
        }

        // Second pass: constellation line segments. Distance from tap
        // to each segment; closest within LINE_HIT_RADIUS wins. We
        // anchor the info-card position at the closest point ON the
        // segment so the card pops up where the tap actually landed
        // on the line, not at one of the endpoint stars.
        //
        // Bounding-box pre-check: a segment whose AABB doesn't overlap
        // the tap-radius circle can't possibly contain a hit, so we
        // skip the point-to-segment distance calc entirely. This makes
        // hover hit-testing (which runs on every MouseMove) cheap
        // enough to keep running even as the catalog of asterisms
        // grows.
        let lines = self.painted_lines.borrow();
        let mut best_line: Option<(f64, &PaintedSegment, Point)> = None;
        for seg in lines.iter() {
            let min_x = seg.p0.x.min(seg.p1.x) - LINE_HIT_RADIUS;
            let max_x = seg.p0.x.max(seg.p1.x) + LINE_HIT_RADIUS;
            let min_y = seg.p0.y.min(seg.p1.y) - LINE_HIT_RADIUS;
            let max_y = seg.p0.y.max(seg.p1.y) + LINE_HIT_RADIUS;
            if tap_pos.x < min_x
                || tap_pos.x > max_x
                || tap_pos.y < min_y
                || tap_pos.y > max_y
            {
                continue;
            }
            let (dist, closest) = point_to_segment_distance(tap_pos, seg.p0, seg.p1);
            if dist > LINE_HIT_RADIUS {
                continue;
            }
            match best_line {
                Some((best_d, _, _)) if dist >= best_d => {}
                _ => best_line = Some((dist, seg, closest)),
            }
        }
        best_line.map(|(_, seg, closest)| {
            let detail = match zodiac_date_range(seg.constellation_name) {
                Some(range) => format!("Constellation · Zodiac · {range}"),
                None => String::from("Constellation"),
            };
            PaintedBody {
                name: seg.constellation_name.to_string(),
                pos: closest,
                // Constellations don't have a meaningful magnitude;
                // use a sentinel that sorts after everything else.
                magnitude: f32::INFINITY,
                detail: Some(detail),
            }
        })
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

/// Shortest distance from point `p` to the line segment `a → b`, plus
/// the closest point on the segment. Used by [`SkyViewWidget::hit_test_tap`]
/// to resolve taps on constellation line segments.
pub(super) fn point_to_segment_distance(p: Point, a: Point, b: Point) -> (f64, Point) {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let len_sq = abx * abx + aby * aby;
    if len_sq < 1e-9 {
        let dx = p.x - a.x;
        let dy = p.y - a.y;
        return ((dx * dx + dy * dy).sqrt(), a);
    }
    let t = (((p.x - a.x) * abx + (p.y - a.y) * aby) / len_sq).clamp(0.0, 1.0);
    let closest = Point::new(a.x + t * abx, a.y + t * aby);
    let dx = p.x - closest.x;
    let dy = p.y - closest.y;
    ((dx * dx + dy * dy).sqrt(), closest)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `t`-clamping branches: point projects past the endpoints should
    /// resolve to the endpoint, not to the extended line.
    #[test]
    fn segment_distance_clamps_to_endpoints() {
        let a = Point::new(0.0, 0.0);
        let b = Point::new(10.0, 0.0);
        // Past the start of the segment.
        let (d, c) = point_to_segment_distance(Point::new(-5.0, 0.0), a, b);
        assert_eq!(d, 5.0);
        assert_eq!((c.x, c.y), (0.0, 0.0));
        // Past the end.
        let (d, c) = point_to_segment_distance(Point::new(20.0, 3.0), a, b);
        assert!((d - ((10.0_f64).hypot(3.0))).abs() < 1e-9);
        assert_eq!((c.x, c.y), (10.0, 0.0));
    }

    /// Perpendicular distance to the interior of the segment is the
    /// y-offset for a horizontal segment.
    #[test]
    fn segment_distance_perpendicular_inside() {
        let a = Point::new(0.0, 0.0);
        let b = Point::new(10.0, 0.0);
        let (d, c) = point_to_segment_distance(Point::new(4.0, 7.0), a, b);
        assert_eq!(d, 7.0);
        assert_eq!((c.x, c.y), (4.0, 0.0));
    }

    /// Degenerate segment (a == b) must still yield a sane distance
    /// (radial from the shared point), not divide by zero.
    #[test]
    fn segment_distance_degenerate_handled() {
        let p = Point::new(3.0, 4.0);
        let (d, c) = point_to_segment_distance(p, Point::new(0.0, 0.0), Point::new(0.0, 0.0));
        assert_eq!(d, 5.0);
        assert_eq!((c.x, c.y), (0.0, 0.0));
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
                    // Idle pointer: nothing to do. Constellation
                    // detection is reticle-driven, not cursor-driven —
                    // moving the mouse around shouldn't pop tooltips
                    // (matches how stars work; they're identified by
                    // the centre reticle, not the cursor).
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

                    // Decompose → increment → recompose. The
                    // previous formulation (`q_world_yaw * view_quat
                    // * q_local_pitch`) looked roll-free on paper
                    // but accumulated small roll under sequences of
                    // diagonal drags (pinned in
                    // `math::tests::alt_zero_projects_to_horizontal_line_after_drags`).
                    // Round-tripping through (yaw, pitch) every
                    // drag guarantees the rebuilt quaternion lives
                    // in the no-roll subspace exactly — any drift
                    // gets discarded.
                    //
                    // dy convention preserved from the previous
                    // handler: drag down (positive dy) → look up
                    // (pitch increases).
                    let fwd = self
                        .view_quat
                        .get()
                        .inverse_transform_vector(&Vector3::new(0.0, 0.0, 1.0));
                    let cur_pitch = fwd.y.clamp(-1.0, 1.0).asin();
                    let cur_yaw = (-fwd.x).atan2(fwd.z);
                    let new_yaw = cur_yaw + (-dx * sensitivity);
                    // Clamp a hair shy of ±π/2 so atan2 stays
                    // well-defined at the next decompose call —
                    // otherwise the user could pin the camera at
                    // the singularity and `cur_yaw` becomes noise.
                    let pitch_cap = std::f64::consts::FRAC_PI_2 - 0.01;
                    let new_pitch =
                        (cur_pitch + dy * sensitivity).clamp(-pitch_cap, pitch_cap);
                    let q_yaw =
                        UnitQuaternion::from_axis_angle(&Vector3::y_axis(), new_yaw);
                    let q_pitch =
                        UnitQuaternion::from_axis_angle(&Vector3::x_axis(), new_pitch);
                    let new_quat = q_pitch * q_yaw;
                    self.view_quat.set(new_quat);
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
                            pos: hit.pos,
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
        let mut painted_lines: Vec<PaintedSegment> = Vec::new();

        // Night-sky backdrop (deep indigo).
        Self::fill_rect(ctx, Rect::new(0.0, 0.0, w, h), Color::from_rgb8(10, 10, 25));

        let center = Point::new(w / 2.0, h * 0.6);
        let focal_length = (w.min(h)) * 0.9;

        // Build the world→view rotation matrix from the quaternion
        // state. Calibration applies as an additional rotation around
        // the world up axis (a compass-offset), composed on the right
        // so its meaning matches the "subtract this much yaw from the
        // incoming compass reading" semantics the Calibrate button
        // implements.
        let cal_offset = self.calibration_yaw.get();
        let q_cal = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), -cal_offset);
        let effective_quat = self.view_quat.get() * q_cal;
        let rot = effective_quat.to_rotation_matrix().into_inner();

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
            // Look up endpoints in the *full* catalog (seed + parsed
            // CSV) so constellation lines can reference extended-catalog
            // stars like Sadalsuud or Kaus Media. BRIGHTEST_STARS alone
            // is the 26-star seed and only covers Orion + Ursa Major.
            let stars = all_stars();
            for line in CONSTELLATION_LINES {
                let from = stars.iter().find(|s| s.id == line.from_id);
                let to = stars.iter().find(|s| s.id == line.to_id);
                if let (Some(from), Some(to)) = (from, to) {
                    let h_from = equatorial_to_horizontal(from.coords, lat, lst);
                    let h_to = equatorial_to_horizontal(to.coords, lat, lst);
                    if let (Some(p_from), Some(p_to)) = (
                        self.project_horizontal(h_from, &rot, center, focal_length),
                        self.project_horizontal(h_to, &rot, center, focal_length),
                    ) {
                        Self::stroke_segment(ctx, p_from, p_to, 1.0, line_color);
                        painted_lines.push(PaintedSegment {
                            constellation_name: line.constellation_name,
                            p0: p_from,
                            p1: p_to,
                        });
                    }
                }
            }
        }

        // Stars. Painted regardless of altitude so the user can pan
        // / tilt down past the horizon and still see the constellations
        // hiding "behind the Earth" — matches Stellarium-style behaviour.
        // The painted alt=0 line + ground strip remain the visual
        // reference for which half is sky and which is ground.
        ctx.set_font(Arc::clone(&self.font));
        for star in all_stars() {
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
        // No below-horizon cull — the Sun at midnight is genuinely useful
        // to find ("where is the Sun right now?") and panning down to see
        // a planet that just set should still resolve it. Behind-camera
        // (z<=0.05) is the only thing project_horizontal skips.
        for body in calculate_solar_system_bodies(self.timestamp_ms.get()) {
            let horiz = equatorial_to_horizontal(body.coords, lat, lst);
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
        // Dim alt=0 horizon line projected across the sky — gives a
        // visual cue for "how far above / below the horizon am I
        // looking?" that the locked-level bottom strip can't convey on
        // its own. Painted before the HUD strips so they sit on top.
        hud::paint_alt_zero_line(ctx, w, h, &rot, center, focal_length);

        hud::paint_horizon_strip(ctx, Arc::clone(&self.font), w, h, &rot, center, focal_length);

        // Altitude ladder along the right edge — like an HUD pitch
        // tape — so the user can see at a glance how far above (or
        // below) the horizon the centre of the screen is pointing.
        // Particularly important now that the horizon is locked level
        // at the bottom of the screen.
        let centre_alt = hud::screen_centre_altitude(&rot);
        hud::paint_altitude_ladder(ctx, Arc::clone(&self.font), w, h, centre_alt);

        // Centre reticle (circle) + name printed below it when a body
        // is actually inside the ring. Lets the user "aim" the reticle
        // at a bright object and read off what it is, reading just
        // below where their eye already is.
        hud::paint_centre_reticle(
            ctx,
            Arc::clone(&self.font),
            w,
            h,
            centre_alt,
            &painted,
            &painted_lines,
        );

        // Sticky info card from a tap. Bodies re-resolve their
        // position from this frame's `painted` set so the card
        // tracks the body as the user pans; constellation hits
        // don't appear in `painted`, so they fall back to the
        // stored screen position from the original tap.
        if let Some(sel) = self.selected.clone() {
            let anchor = painted
                .iter()
                .find(|p| p.name == sel.name)
                .map(|b| b.pos)
                .unwrap_or(sel.pos);
            hud::paint_info_card(
                ctx,
                Arc::clone(&self.font),
                anchor,
                Rect::new(0.0, 0.0, w, h),
                &sel,
            );
        }

        // Promote this frame's projections to the cache for the next tap.
        self.painted_bodies.replace(painted);
        self.painted_lines.replace(painted_lines);
    }
}

