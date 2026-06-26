//! On-sky "where do I point?" finder for the search feature.
//!
//! Once the user picks a search target ([`crate::search::SearchTarget`]),
//! the sky view calls [`paint`] every frame. It resolves the target to a
//! live direction, then draws:
//!
//! * a pulsing highlight ring when the target is on-screen,
//! * a single cohesive "pin" — a ring around the reticle with an arrow
//!   morphing out of its edge toward the target (including a "behind you"
//!   direction when it's behind the camera), and
//! * a proximity meter: a round-capped arc just outside the ring that
//!   starts invisible when far, grows from the point opposite the arrow
//!   around both sides, and fully encircles the ring on arrival. As the
//!   meter's ends close in on the arrow, the arrow retracts into the ring
//!   so the two never touch.
//!
//! Pulled into its own module so `sky_view.rs` stays under the workspace
//! 800-line guardrail. The geometry helpers [`turn_guidance`],
//! [`meter_half_sweep`], and [`arrow_scale`] are pure and unit-tested.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::geometry::Point;
use agg_gui::LineCap;
use nalgebra::Matrix3;

use crate::math::{equatorial_to_horizontal, horizontal_to_cartesian, HorizontalCoords};
use crate::search::{resolve_target_coords, SearchTarget};

/// Below this separation the target counts as "found": the meter snaps to a
/// full ring and the arrow fully retracts into it.
const FOUND_THRESHOLD_RAD: f64 = 0.035; // ~2°

/// Separation at which the proximity meter reads empty. Anything wider than
/// this maps to the same "far" state.
const GAUGE_FULL_SCALE_RAD: f64 = std::f64::consts::FRAC_PI_2; // 90°

/// Radius (logical px) of the pin's ring centreline around the reticle.
const RING_RADIUS: f64 = 22.0;

/// Stroke width (logical px) of the pin's ring.
const RING_STROKE_W: f64 = 3.5;

/// How far the arrow tip extends beyond the ring centreline at full size.
const ARROW_LEN: f64 = 13.0;

/// Half-width (logical px) of the arrow base at full size.
const ARROW_HALF_BASE: f64 = 8.0;

/// Stroke width (logical px) of the proximity meter arc.
const METER_STROKE_W: f64 = 3.0;

/// Gap (logical px) between the ring's outer edge and the meter arc.
const METER_GAP: f64 = 4.0;

/// Radius (logical px) at which the proximity meter arc is stroked — just
/// outside the ring.
const METER_RADIUS: f64 =
    RING_RADIUS + RING_STROKE_W * 0.5 + METER_GAP + METER_STROKE_W * 0.5;

/// Extra angular clearance (radians) kept between the meter's ends and the
/// arrow so the round caps never visually kiss the arrow.
const ARROW_CLEAR_MARGIN: f64 = 0.18;

/// Angular deltas needed to bring a target to the centre of the view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurnGuidance {
    /// Total great-circle separation between view centre and the target.
    pub sep_rad: f64,
    /// Wrapped `target_az - view_az` in `[-π, π]`. Positive = the target
    /// is clockwise of the current heading, i.e. "turn right".
    pub delta_az: f64,
    /// `target_alt - centre_alt`. Positive = the target is higher, i.e.
    /// "tilt up".
    pub delta_alt: f64,
}

/// Compute the turn / tilt guidance from the current view direction
/// (`view_az`, `centre_alt`, both radians) to a target in horizontal
/// coordinates. Pure so it can be unit-tested without a render context.
pub fn turn_guidance(target: HorizontalCoords, view_az: f64, centre_alt: f64) -> TurnGuidance {
    TurnGuidance {
        sep_rad: angular_separation(target.alt, target.az, centre_alt, view_az),
        delta_az: wrap_pi(target.az - view_az),
        delta_alt: target.alt - centre_alt,
    }
}

/// Great-circle angle between two horizontal directions (spherical law of
/// cosines). Clamped against float drift so `acos` never sees > 1.
fn angular_separation(alt1: f64, az1: f64, alt2: f64, az2: f64) -> f64 {
    let cos_sep =
        alt1.sin() * alt2.sin() + alt1.cos() * alt2.cos() * (az1 - az2).cos();
    cos_sep.clamp(-1.0, 1.0).acos()
}

/// Wrap an angle into `[-π, π]`.
fn wrap_pi(mut a: f64) -> f64 {
    use std::f64::consts::PI;
    while a > PI {
        a -= 2.0 * PI;
    }
    while a < -PI {
        a += 2.0 * PI;
    }
    a
}

/// Paint the finder for the currently-selected target, if any. No-op when
/// no target is set or it can't be resolved.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint(
    ctx: &mut dyn DrawCtx,
    w: f64,
    h: f64,
    rot: &Matrix3<f64>,
    center: Point,
    focal_length: f64,
    target_cell: &Rc<RefCell<Option<SearchTarget>>>,
    lat: f64,
    lst: f64,
    now_ms: i64,
) {
    let borrow = target_cell.borrow();
    let Some(target) = borrow.as_ref() else {
        return;
    };
    let Some(coords) = resolve_target_coords(target, now_ms) else {
        return;
    };
    let target_horiz = equatorial_to_horizontal(coords, lat, lst);

    // Target direction in camera space (x = right, y = up, z = forward).
    let v_cart = horizontal_to_cartesian(target_horiz);
    let v_rot = rot * v_cart;

    // Current view heading + pitch from the camera-forward row of `rot`
    // (same extraction the altitude HUD uses), so guidance stays in lock-
    // step with what's actually drawn this frame.
    let cam_forward = nalgebra::Vector3::new(rot[(2, 0)], rot[(2, 1)], rot[(2, 2)]);
    let cam_len = cam_forward.norm().max(1e-9);
    let view_az = cam_forward.x.atan2(cam_forward.z);
    let centre_alt = (cam_forward.y / cam_len).asin();
    let guidance = turn_guidance(target_horiz, view_az, centre_alt);

    let accent = Color::from_rgb8(120, 255, 170);

    // Screen-space direction from the reticle toward the target, plus the
    // on-screen highlight when the object itself is visible.
    let angle = if v_rot.z > 0.05 {
        let screen = Point::new(
            center.x + (v_rot.x / v_rot.z) * focal_length,
            center.y + (v_rot.y / v_rot.z) * focal_length,
        );
        let on_screen =
            screen.x >= 0.0 && screen.x <= w && screen.y >= 0.0 && screen.y <= h;
        if on_screen {
            paint_highlight_ring(ctx, screen, now_ms, accent);
        }
        (screen.y - center.y).atan2(screen.x - center.x)
    } else {
        // Behind the camera: point along the target's lateral offset.
        v_rot.y.atan2(v_rot.x)
    };

    // Closeness 0..1. Snap to "arrived" within the found threshold so the
    // meter completes and the arrow fully retracts instead of hovering at
    // ~99%.
    let prox = if guidance.sep_rad < FOUND_THRESHOLD_RAD {
        1.0
    } else {
        proximity(guidance.sep_rad)
    };

    // The meter grows around the outside as you approach; the pin (ring +
    // arrow) sits on top so the arrow tip stays crisp over the meter. The
    // arrow retracts as the meter's ends close in on it.
    let a_scale = arrow_scale(prox, RING_RADIUS, ARROW_HALF_BASE, ARROW_CLEAR_MARGIN);
    paint_meter(ctx, center, angle, prox, meter_color(prox));
    paint_pin(ctx, center, angle, a_scale, accent);
}

/// Map a great-circle separation to a 0..1 "closeness", saturating at
/// [`GAUGE_FULL_SCALE_RAD`]. 1.0 = on target, 0.0 = far away.
fn proximity(sep_rad: f64) -> f64 {
    1.0 - (sep_rad / GAUGE_FULL_SCALE_RAD).clamp(0.0, 1.0)
}

/// Half-sweep (radians, per side) of the proximity meter for a given
/// closeness. 0 when far (invisible), `PI` when arrived (a full ring).
fn meter_half_sweep(prox: f64) -> f64 {
    prox.clamp(0.0, 1.0) * std::f64::consts::PI
}

/// Map closeness 0..1 to a cold→hot colour for the meter: cold blue when far
/// from the target, ramping through cyan/green/amber to hot red on arrival.
fn meter_color(prox: f64) -> Color {
    // Stops along a perceptual cold→hot ramp (closeness, RGB).
    const STOPS: [(f64, (u8, u8, u8)); 5] = [
        (0.0, (40, 110, 255)),  // cold blue (far)
        (0.25, (40, 210, 230)), // cyan
        (0.5, (70, 220, 90)),   // green
        (0.75, (240, 200, 60)), // amber
        (1.0, (255, 70, 60)),   // hot red (on target)
    ];
    let p = prox.clamp(0.0, 1.0);
    let mut i = 0;
    while i + 1 < STOPS.len() && p > STOPS[i + 1].0 {
        i += 1;
    }
    let (t0, c0) = STOPS[i];
    let (t1, c1) = STOPS[i + 1];
    let f = if (t1 - t0).abs() < 1e-9 {
        0.0
    } else {
        (p - t0) / (t1 - t0)
    };
    let lerp = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * f).round() as u8;
    Color::from_rgb8(lerp(c0.0, c1.0), lerp(c0.1, c1.1), lerp(c0.2, c1.2))
}

/// Size factor (0..1) for the arrow. 1.0 while the meter's ends are well
/// clear of the arrow; ramps to 0.0 as the growing meter closes in, so the
/// meter never touches the arrow. `prox` is closeness 0..1;
/// `ring_radius`/`half_base` set the arrow's angular width and `margin` is
/// extra clearance (radians).
fn arrow_scale(prox: f64, ring_radius: f64, half_base: f64, margin: f64) -> f64 {
    let prox = prox.clamp(0.0, 1.0);
    // Angular gap (radians) from each meter end to the arrow centre.
    let gap = (1.0 - prox) * std::f64::consts::PI;
    // Arrow's angular half-width at the ring.
    let aw = (half_base / ring_radius.max(1e-6)).atan();
    if aw <= 1e-9 {
        return 1.0;
    }
    ((gap - margin) / aw).clamp(0.0, 1.0)
}

/// Proximity meter: a single round-capped arc stroked just outside the ring.
/// It is centred on the point opposite the arrow and grows symmetrically as
/// `prox` rises, closing into a full ring at arrival.
fn paint_meter(ctx: &mut dyn DrawCtx, center: Point, angle: f64, prox: f64, color: Color) {
    use std::f64::consts::PI;
    let half = meter_half_sweep(prox);
    if half <= 1e-3 {
        return;
    }
    let far = angle + PI; // diametrically opposite the arrow
    ctx.set_line_cap(LineCap::Round);
    ctx.set_line_width(METER_STROKE_W);
    ctx.set_stroke_color(color);
    ctx.begin_path();
    // `ccw = true` sweeps the *short* arc from `far - half` to `far + half`
    // (centred on the far point). With `false` the underlying arc generator
    // pushes the start past the end and draws the complement — the long way
    // around — which would centre the fill on the arrow and shrink it as you
    // approach. We want the meter to grow out of the far side toward the
    // arrow, so sweep the short way.
    ctx.arc_to(center.x, center.y, METER_RADIUS, far - half, far + half, true);
    ctx.stroke();
}

/// Draw the pin: a stroked ring with the arrow morphing out of its edge
/// toward `angle`. `scale` (0..1) shrinks the arrow back into the ring so it
/// vanishes flush at 0.
fn paint_pin(ctx: &mut dyn DrawCtx, center: Point, angle: f64, scale: f64, color: Color) {
    // Ring.
    ctx.set_line_cap(LineCap::Round);
    ctx.set_line_width(RING_STROKE_W);
    ctx.set_stroke_color(color);
    ctx.begin_path();
    ctx.circle(center.x, center.y, RING_RADIUS);
    ctx.stroke();

    let scale = scale.clamp(0.0, 1.0);
    if scale <= 1e-3 {
        return;
    }

    // Arrow, as a filled shape whose base is buried in the ring band (so
    // there is no seam) and whose tip extends outward. Sides curve via
    // quad_to for a soft morph into the circle.
    let (c, s) = (angle.cos(), angle.sin());
    let dir = Point::new(c, s);
    let perp = Point::new(-s, c);

    let tip_dist = RING_RADIUS + ARROW_LEN * scale;
    let base_dist = RING_RADIUS - RING_STROKE_W * 0.5; // buried in the ring
    let half_base = ARROW_HALF_BASE * scale;

    let at = |along: f64, across: f64| {
        Point::new(
            center.x + dir.x * along + perp.x * across,
            center.y + dir.y * along + perp.y * across,
        )
    };

    let tip = at(tip_dist, 0.0);
    let base_l = at(base_dist, half_base);
    let base_r = at(base_dist, -half_base);
    // Control points partway out, flared to the arrow's full width, give the
    // sides a gentle taper that reads as a continuation of the ring.
    let ctrl_l = at(RING_RADIUS + ARROW_LEN * scale * 0.35, half_base * 0.95);
    let ctrl_r = at(RING_RADIUS + ARROW_LEN * scale * 0.35, -half_base * 0.95);

    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.move_to(base_l.x, base_l.y);
    ctx.quad_to(ctrl_l.x, ctrl_l.y, tip.x, tip.y);
    ctx.quad_to(ctrl_r.x, ctrl_r.y, base_r.x, base_r.y);
    ctx.line_to(base_l.x, base_l.y);
    ctx.close_path();
    ctx.fill();
}

/// Small fixed-size ring centred on an on-screen target that pulses by
/// fading in and out (no size change).
fn paint_highlight_ring(ctx: &mut dyn DrawCtx, p: Point, now_ms: i64, color: Color) {
    use std::f64::consts::PI;
    // Smooth 0..1 pulse on a ~1.1 s cycle, mapped to an alpha range that
    // never fully disappears so the marker stays legible.
    let phase = (now_ms.rem_euclid(1100) as f64) / 1100.0;
    let pulse = 0.5 - 0.5 * (phase * 2.0 * PI).cos();
    let alpha = (0.35 + pulse * 0.65) as f32;
    let r = 6.0;
    agg_gui::animation::request_draw_without_invalidation();

    ctx.set_stroke_color(color.with_alpha(alpha));
    ctx.set_line_width(2.0);
    ctx.begin_path();
    ctx.circle(p.x, p.y, r);
    ctx.stroke();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn pointing_at_target_gives_zero_separation() {
        let target = HorizontalCoords {
            alt: 0.3,
            az: 1.2,
        };
        let g = turn_guidance(target, 1.2, 0.3);
        assert!(g.sep_rad < 1e-9, "sep should be ~0, got {}", g.sep_rad);
        assert!(g.delta_az.abs() < 1e-9);
        assert!(g.delta_alt.abs() < 1e-9);
    }

    #[test]
    fn target_to_the_right_is_positive_delta_az() {
        // Target azimuth slightly greater (clockwise) than the view.
        let target = HorizontalCoords { alt: 0.0, az: 1.0 };
        let g = turn_guidance(target, 0.8, 0.0);
        assert!(g.delta_az > 0.0, "expected turn-right, got {}", g.delta_az);
    }

    #[test]
    fn target_above_is_positive_delta_alt() {
        let target = HorizontalCoords { alt: 0.5, az: 0.0 };
        let g = turn_guidance(target, 0.0, 0.2);
        assert!(g.delta_alt > 0.0, "expected tilt-up, got {}", g.delta_alt);
        assert!((g.delta_alt - 0.3).abs() < 1e-9);
    }

    #[test]
    fn delta_az_takes_short_way_across_the_seam() {
        // View near 350°, target near 10°: shortest turn is +20° (right),
        // not -340°.
        let target = HorizontalCoords {
            alt: 0.0,
            az: 10.0_f64.to_radians(),
        };
        let g = turn_guidance(target, 350.0_f64.to_radians(), 0.0);
        assert!(g.delta_az > 0.0, "should turn right across the seam");
        assert!(
            (g.delta_az - 20.0_f64.to_radians()).abs() < 1e-9,
            "expected +20°, got {}°",
            g.delta_az.to_degrees()
        );
    }

    #[test]
    fn separation_is_symmetric_and_bounded() {
        let a = angular_separation(0.1, 0.2, -0.4, 3.0);
        let b = angular_separation(-0.4, 3.0, 0.1, 0.2);
        assert!((a - b).abs() < 1e-12);
        assert!((0.0..=PI).contains(&a));
    }

    #[test]
    fn meter_half_sweep_spans_zero_to_pi() {
        assert!(meter_half_sweep(0.0).abs() < 1e-12, "far = no sweep");
        assert!(
            (meter_half_sweep(1.0) - PI).abs() < 1e-12,
            "arrived = full half-sweep of PI (a complete ring)"
        );
        // Clamped outside 0..1.
        assert!(meter_half_sweep(-0.5).abs() < 1e-12);
        assert!((meter_half_sweep(2.0) - PI).abs() < 1e-12);
        // Monotonic non-decreasing.
        let mut prev = -1.0;
        for i in 0..=10 {
            let v = meter_half_sweep(i as f64 / 10.0);
            assert!(v >= prev, "half-sweep must be non-decreasing");
            prev = v;
        }
    }

    #[test]
    fn arrow_is_full_when_meter_far_and_gone_when_arrived() {
        // Far away: meter invisible, arrow at full size.
        assert!(
            (arrow_scale(0.0, RING_RADIUS, ARROW_HALF_BASE, ARROW_CLEAR_MARGIN) - 1.0).abs()
                < 1e-12,
            "arrow full when meter is far"
        );
        // Arrived: arrow fully retracted so the closed meter never touches it.
        assert!(
            arrow_scale(1.0, RING_RADIUS, ARROW_HALF_BASE, ARROW_CLEAR_MARGIN).abs() < 1e-12,
            "arrow gone on arrival"
        );
    }

    #[test]
    fn meter_color_runs_cold_blue_to_hot_red() {
        let cold = meter_color(0.0);
        let hot = meter_color(1.0);
        // Far end is blue-dominant (cold).
        assert!(
            cold.b > cold.r && cold.b > cold.g,
            "far meter should be cold blue, got {cold:?}"
        );
        // Near end is red-dominant (hot).
        assert!(
            hot.r > hot.b && hot.r > hot.g,
            "on-target meter should be hot red, got {hot:?}"
        );
    }

    #[test]
    fn arrow_scale_is_monotonically_non_increasing_in_proximity() {
        let mut prev = f64::INFINITY;
        for i in 0..=20 {
            let prox = i as f64 / 20.0;
            let s = arrow_scale(prox, RING_RADIUS, ARROW_HALF_BASE, ARROW_CLEAR_MARGIN);
            assert!(
                s <= prev + 1e-12,
                "arrow_scale must not grow as proximity rises: prox={prox}, s={s}, prev={prev}"
            );
            assert!((0.0..=1.0).contains(&s), "arrow_scale stays in 0..1");
            prev = s;
        }
    }
}
