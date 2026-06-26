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

mod geometry;
mod hud;
mod moon_phase;
mod pan;
mod target_finder;

use geometry::{draw_text, fill_disc, fill_rect, point_to_segment_distance, stroke_segment};

use crate::math::{
    equatorial_to_horizontal, format_rise_set, horizontal_to_cartesian, rise_set_times,
    HorizontalCoords, STANDARD_REFRACTION_ALT_RAD, SUN_HORIZON_ALT_RAD,
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
    /// Pre-formatted rise / set string for the reticle card, in the
    /// observer's local time (DST applied). `None` for things that
    /// don't have meaningful rise/set (e.g. constellations).
    /// Format examples: `"Rises 18:42 · Sets 06:13"`, `"Always up"`,
    /// `"Below horizon today"`.
    pub rise_set: Option<String>,
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

    /// Whether the device-orientation (compass) channel is driving the
    /// view. When `true` the phone's sensors aim the camera, so a tap is
    /// no longer a pointing gesture — we suppress the tap-to-identify
    /// info card to avoid surprise cards popping up as the user pans the
    /// sky by physically turning. When `false` (desktop / compass off)
    /// tapping resolves the nearest body and shows its info card.
    use_device_orientation: Rc<Cell<bool>>,

    /// Visibility of the on-screen control chrome (the mobile left-edge
    /// button rail). A tap on empty sky — one that hits no body and no
    /// constellation line — toggles this so the user can clear the
    /// overlays for an unobstructed view and tap again to bring them
    /// back. Nothing reads it on desktop (there's no rail), so the tap
    /// is a harmless no-op there.
    show_controls: Rc<Cell<bool>>,

    /// Closure that returns the device's UTC offset in minutes east of
    /// UTC (DST-aware), supplied by the platform shell. Used to format
    /// rise / set times in the reticle card as the user's local clock,
    /// not UTC. Stored as `Rc<dyn Fn>` so SkyView doesn't need to know
    /// the concrete `AstronomerPlatform` impl.
    local_offset_fn: Rc<dyn Fn() -> i32>,

    /// Shared toast cell. The control panel writes feedback messages
    /// here ("Calibrated", "Constellations on", …); this widget reads
    /// and paints the card on top of the sky.
    toast: crate::toast::ToastCell,

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
    /// Active search target (set by the search panel). When `Some`, the
    /// finder overlay points the user toward it. See `target_finder`.
    search_target: Rc<RefCell<Option<crate::search::SearchTarget>>>,
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
        use_device_orientation: Rc<Cell<bool>>,
        show_controls: Rc<Cell<bool>>,
        local_offset_fn: Rc<dyn Fn() -> i32>,
        toast: crate::toast::ToastCell,
        search_target: Rc<RefCell<Option<crate::search::SearchTarget>>>,
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
            use_device_orientation,
            show_controls,
            local_offset_fn,
            toast,
            down: None,
            painted_bodies: RefCell::new(Vec::new()),
            painted_lines: RefCell::new(Vec::new()),
            selected: None,
            search_target,
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
                rise_set: None,
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
                if down.is_drag && !self.use_device_orientation.get() {
                    // Grab-to-pan: rotate the whole sky rigidly so the world
                    // point under the finger stays pinned to it, the way
                    // Google Sky Map's manual control does. Uses the same
                    // pin-hole projection geometry as `paint` (center +
                    // focal_length); see `pan::drag_view_quat`.
                    //
                    // Skipped while the compass (device-orientation) channel
                    // is driving the view: the phone's sensors aim the
                    // camera there, so a finger drag must not fight them.
                    let center =
                        Point::new(self.bounds.width / 2.0, self.bounds.height * 0.6);
                    let focal_length =
                        (self.bounds.width.min(self.bounds.height)) * 0.9;
                    let new_quat = pan::drag_view_quat(
                        self.view_quat.get(),
                        down.last,
                        *pos,
                        center,
                        focal_length,
                    );
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
                    // With the compass driving the view, the user aims by
                    // physically turning the phone, so taps are not a
                    // pointing gesture — skip tap-to-identify and treat
                    // every tap as a chrome toggle / card dismiss. When
                    // the compass is off, tapping resolves the nearest
                    // body and shows its info card.
                    let compass_on = self.use_device_orientation.get();
                    let hit = if compass_on { None } else { self.hit_test_tap(*pos) };
                    if let Some(hit) = hit {
                        // Tap landed on a body / constellation line: pin
                        // its info card.
                        self.selected = Some(Selection {
                            name: hit.name,
                            magnitude: hit.magnitude,
                            detail: hit.detail,
                            pos: hit.pos,
                        });
                    } else if self.selected.is_some() {
                        // Tap on empty sky while a card is up: first tap
                        // dismisses the card (don't also toggle the
                        // chrome — one gesture, one effect).
                        self.selected = None;
                    } else {
                        // Tap on truly empty sky (between the stars):
                        // toggle the control chrome so the user can get
                        // an unobstructed view and tap again to restore
                        // it.
                        self.show_controls.set(!self.show_controls.get());
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
        fill_rect(ctx, Rect::new(0.0, 0.0, w, h), Color::from_rgb8(10, 10, 25));

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
                        stroke_segment(ctx, p_from, p_to, 1.0, line_color);
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
        let now_ms = self.timestamp_ms.get();
        let local_offset = (self.local_offset_fn)();
        let lng_rad = self.longitude.get().to_radians();
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
            fill_disc(ctx, pos, radius, color);

            if star.magnitude < 1.0 {
                draw_text(
                    ctx,
                    Point::new(pos.x + radius + 3.0, pos.y - 3.0),
                    9.0,
                    Color::from_rgba8(220, 220, 255, 180),
                    star.name,
                );
            }

            let rs = rise_set_times(
                star.coords,
                lat,
                lng_rad,
                now_ms,
                STANDARD_REFRACTION_ALT_RAD,
            );
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
                rise_set: Some(format_rise_set(rs, local_offset)),
            });
        }

        // Solar System bodies. Render brighter / larger discs for the body
        // sizes the user cares about (Sun, Moon big; Venus + Jupiter
        // notably brighter than fixed stars; the others sit between).
        // No below-horizon cull — the Sun at midnight is genuinely useful
        // to find ("where is the Sun right now?") and panning down to see
        // a planet that just set should still resolve it. Behind-camera
        // (z<=0.05) is the only thing project_horizontal skips.
        let bodies = calculate_solar_system_bodies(now_ms);
        // Pull the Sun's coords out once so the Moon-phase painter can
        // compute the bright-limb direction without re-running the
        // ephemeris.
        let sun_coords = bodies.iter().find(|b| b.name == "Sun").map(|b| b.coords);
        for body in &bodies {
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
            // Soft glow halo for the bodies that deserve to "pop".
            if body.name == "Sun" {
                fill_disc(ctx, pos, radius + 6.0, Color::from_rgba8(255, 200, 50, 60));
            } else if body.name == "Moon" {
                fill_disc(ctx, pos, radius + 3.0, Color::from_rgba8(220, 220, 240, 50));
            } else if body.name == "Venus" || body.name == "Jupiter" {
                fill_disc(ctx, pos, radius + 3.0, Color::from_rgba8(255, 240, 200, 60));
            }

            // The Moon gets a phase rendering; everything else is a
            // plain coloured disc.
            if body.name == "Moon" {
                let phase_info = sun_coords.map(|sc| {
                    moon_phase::moon_phase_info(sc, body.coords, lat, lst, &rot)
                });
                moon_phase::fill_moon_phase(ctx, pos, radius, phase_info);
            } else {
                fill_disc(ctx, pos, radius, body.color);
            }
            draw_text(
                ctx,
                Point::new(pos.x + radius + 4.0, pos.y - 4.0),
                12.0,
                Color::from_rgb8(255, 255, 255),
                body.name,
            );

            let detail = if body.name == "Sun" {
                Some(format!("Solar System · mag {:.1}", body.magnitude))
            } else if body.name == "Moon" {
                let illum = sun_coords
                    .map(|sc| moon_phase::moon_illumination(sc, body.coords))
                    .unwrap_or(0.0);
                Some(format!(
                    "Solar System · mag {:.1} · {:.0}% lit",
                    body.magnitude,
                    illum * 100.0
                ))
            } else {
                Some(format!(
                    "Planet · mag {:.1} · alt {:+.0}° · az {:.0}°",
                    body.magnitude,
                    horiz.alt.to_degrees(),
                    horiz.az.to_degrees(),
                ))
            };
            // Sun gets refraction + apparent-radius horizon offset; the
            // rest just refraction. Rise/set is pre-formatted in the
            // user's local time so the card doesn't have to know the
            // platform offset.
            let horizon_alt = if body.name == "Sun" {
                SUN_HORIZON_ALT_RAD
            } else {
                STANDARD_REFRACTION_ALT_RAD
            };
            let rs = rise_set_times(body.coords, lat, lng_rad, now_ms, horizon_alt);
            painted.push(PaintedBody {
                name: body.name.to_string(),
                pos,
                magnitude: body.magnitude,
                detail,
                rise_set: Some(format_rise_set(rs, local_offset)),
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

        // Search finder overlay — arrow / ring / distance toward the
        // active search target, if any.
        target_finder::paint(
            ctx,
            w,
            h,
            &rot,
            center,
            focal_length,
            &self.search_target,
            lat,
            lst,
            now_ms,
        );

        // Toast on top of everything — feedback for icon-only mobile
        // taps. `now_ms` is the projection clock, which the WASM
        // shell pumps every frame, so the fade is in real time.
        let toast_state = self.toast.borrow().clone();
        hud::paint_toast(ctx, Arc::clone(&self.font), w, h, &toast_state, now_ms);
    }
}

