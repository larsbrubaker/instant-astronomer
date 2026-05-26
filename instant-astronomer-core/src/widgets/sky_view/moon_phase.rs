//! Moon phase computation + painter.
//!
//! Pulled out of `sky_view.rs` to keep that file under the 800-line
//! guardrail; the entry point is [`fill_moon_phase`]. Two pieces of
//! information are required at paint time:
//!
//! - the **illuminated fraction** `k ∈ [0, 1]` (0 = new, 1 = full),
//!   derived from the geocentric elongation between Sun and Moon;
//! - the **screen-space direction to the Sun**, which gives the
//!   orientation of the bright limb. The terminator is perpendicular
//!   to this direction.
//!
//! [`moon_phase_info`] bundles both, and [`fill_moon_phase`] does the
//! drawing — including the < 10 % illuminated "outline only" fallback
//! so the dark Moon stays visible against the night sky.

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::geometry::Point;

use crate::math::{equatorial_to_horizontal, horizontal_to_cartesian, EquatorialCoords};

/// Phase information for the Moon, gathered once per frame so the
/// painter has everything it needs without re-running the ephemeris.
#[derive(Debug, Clone, Copy)]
pub(super) struct MoonPhaseInfo {
    /// Illuminated fraction in `[0, 1]`. 0 = new, 1 = full.
    pub illumination: f64,
    /// Unit vector pointing from the Moon toward the Sun in screen
    /// space (Y-up). The terminator ellipse's major axis is
    /// perpendicular to this; the bright limb faces this way.
    pub sun_dir: (f64, f64),
}

/// Illuminated fraction of the Moon in `[0, 1]`. From the angle
/// between the Sun's and Moon's geocentric directions (elongation):
/// `k = (1 - cos ψ) / 2`, exact for the simple Earth-Sun-Moon model
/// and well within naked-eye accuracy.
pub(super) fn moon_illumination(sun: EquatorialCoords, moon: EquatorialCoords) -> f64 {
    let cos_e = sun.dec.sin() * moon.dec.sin()
        + sun.dec.cos() * moon.dec.cos() * (sun.ra - moon.ra).cos();
    let cos_e = cos_e.clamp(-1.0, 1.0);
    ((1.0 - cos_e) / 2.0).clamp(0.0, 1.0)
}

/// Bundle illumination + screen-space sun direction for the painter.
///
/// **The right math here is non-obvious.** The user's bug report was
/// "the lit side rotates as I move my phone" — symptom of using the
/// raw camera-frame `(x, y)` of the moon→sun 3-D chord vector. That
/// approximation only matches the on-screen direction when the moon
/// is at the principal point; off-centre, the perspective divide
/// `(x/z, y/z)` skews the projected direction by a factor that
/// changes with camera orientation.
///
/// The exact fix is to apply the Jacobian of the perspective
/// projection at the moon's view-frame position to the tangent
/// direction at the moon pointing toward the sun:
///
/// 1. `T = S − (S·M̂)M̂` — sun direction projected into the moon's
///    tangent plane (a unit-celestial-sphere quantity, so the sun's
///    distance and which hemisphere it's in don't matter).
/// 2. Rotate `T` and `M` through the camera matrix.
/// 3. `dx_screen ≈ (T_x − M_x · T_z / M_z) / M_z` — and the same for
///    `y`. That's the projection Jacobian acting on the tangent.
///
/// This matches the actual moon→sun vector on screen at any camera
/// orientation, so the lit side stays locked to the real sky.
pub(super) fn moon_phase_info(
    sun: EquatorialCoords,
    moon: EquatorialCoords,
    lat_rad: f64,
    lst_rad: f64,
    rot: &nalgebra::Matrix3<f64>,
) -> MoonPhaseInfo {
    let illum = moon_illumination(sun, moon);
    let sun_h = equatorial_to_horizontal(sun, lat_rad, lst_rad);
    let moon_h = equatorial_to_horizontal(moon, lat_rad, lst_rad);
    let sun_cart = horizontal_to_cartesian(sun_h);
    let moon_cart = horizontal_to_cartesian(moon_h);

    // Tangent direction at the moon pointing toward the sun on the
    // unit celestial sphere. `moon_cart.dot(&moon_cart) = 1` for the
    // unit sphere but we divide anyway so the helper survives a
    // future caller that passes a non-unit Moon vector.
    let dot = sun_cart.dot(&moon_cart);
    let moon_norm_sq = moon_cart.dot(&moon_cart);
    let tangent_world = sun_cart - (dot / moon_norm_sq) * moon_cart;
    let tangent_view = rot * tangent_world;
    let moon_view = rot * moon_cart;
    // Projection Jacobian at the moon, applied to the tangent.
    // `mz.max(epsilon)` is a guard — the moon is in front of the
    // camera whenever we're drawing it, so this only matters in
    // degenerate test setups.
    let mz = moon_view.z.max(1e-6);
    let dx = (tangent_view.x - moon_view.x * tangent_view.z / mz) / mz;
    let dy = (tangent_view.y - moon_view.y * tangent_view.z / mz) / mz;

    let len = (dx * dx + dy * dy).sqrt();
    let sun_dir = if len > 1e-9 {
        (dx / len, dy / len)
    } else {
        (1.0, 0.0)
    };
    MoonPhaseInfo {
        illumination: illum,
        sun_dir,
    }
}

/// Paint the Moon with its current phase. The lit region is a classic
/// lune: half of the Moon's circumference on the Sun-facing side, plus
/// the terminator (an ellipse arc whose semi-minor axis along the Sun
/// direction is `r * (2k - 1)`). When the Moon is more than 90 % new
/// (less than 10 % illuminated) we drop the lit fill entirely and
/// render the disc outline so the dark Moon still tells the user
/// where it is.
pub(super) fn fill_moon_phase(
    ctx: &mut dyn DrawCtx,
    pos: Point,
    r: f64,
    info: Option<MoonPhaseInfo>,
) {
    let Some(info) = info else {
        // No Sun coords available — fall back to a plain bright disc.
        ctx.set_fill_color(Color::from_rgb8(220, 220, 240));
        ctx.begin_path();
        ctx.circle(pos.x, pos.y, r);
        ctx.fill();
        return;
    };
    let bright = Color::from_rgb8(230, 230, 245);
    let outline = Color::from_rgba8(220, 220, 240, 200);

    if info.illumination < 0.10 {
        // Near-new: nothing lit worth filling. Outline ring keeps the
        // body locatable against the night sky.
        ctx.set_stroke_color(outline);
        ctx.set_line_width(1.2);
        ctx.begin_path();
        ctx.circle(pos.x, pos.y, r);
        ctx.stroke();
        return;
    }
    if info.illumination > 0.99 {
        // Full: simpler to just fill a disc than to build a
        // degenerate path.
        ctx.set_fill_color(bright);
        ctx.begin_path();
        ctx.circle(pos.x, pos.y, r);
        ctx.fill();
        return;
    }

    // Lit region path. See `lit_region_path` for the geometry — kept
    // factored out so a polygon-area test can verify the lit fraction
    // matches `k`. (Earlier sign-bug regression had the terminator
    // bulging the wrong way: gibbous moons rendered as if they were
    // crescents and vice versa.)
    let path = lit_region_path(pos, r, info);
    ctx.set_fill_color(bright);
    ctx.begin_path();
    for (i, p) in path.iter().enumerate() {
        if i == 0 {
            ctx.move_to(p.x, p.y);
        } else {
            ctx.line_to(p.x, p.y);
        }
    }
    ctx.fill();
    // Outline the full disc so the dark limb is still discernible
    // even at near-quarter phase.
    ctx.set_stroke_color(outline);
    ctx.set_line_width(0.8);
    ctx.begin_path();
    ctx.circle(pos.x, pos.y, r);
    ctx.stroke();
}

/// Build the polygon vertices for the Moon's lit region as a closed
/// loop. Two parametric arcs share the same endpoints (the "horns"):
///
/// - **Bright limb** (θ ∈ [-π/2, +π/2]): half of the moon's
///   circumference on the Sun-facing side. Traces from one horn,
///   through the sub-solar point at `pos + r·sun_dir`, to the other.
/// - **Terminator** (θ ∈ [+π/2, -π/2]): an ellipse arc whose
///   semi-major axis is `r` along the terminator direction
///   (perpendicular to Sun) and whose **signed** semi-minor axis is
///   `r·(1 - 2k)` along the Sun direction. With `k = illumination`,
///   apex of the terminator sits at `pos + r·(1-2k)·sun_dir`:
///   * `k > 0.5` (gibbous): `(1 - 2k) < 0` → apex on the **anti-Sun**
///     side → terminator bulges into the dark hemisphere → lit
///     region covers more than half the disc.
///   * `k = 0.5`: apex at disc centre → terminator is a straight
///     diameter → exactly half lit.
///   * `k < 0.5` (crescent): `(1 - 2k) > 0` → apex on the **Sun**
///     side → terminator carves into the bright hemisphere → lit
///     region is a thin lune covering less than half.
///
/// Earlier code had `(2k - 1)` (inverted sign) and rendered gibbous
/// as crescent and vice versa — pinned by `polygon_area_matches_k`.
pub(super) fn lit_region_path(pos: Point, r: f64, info: MoonPhaseInfo) -> Vec<Point> {
    const SAMPLES: usize = 48;
    let (sx, sy) = info.sun_dir;
    // Terminator direction: rotate sun_dir 90° CCW.
    let (tx, ty) = (-sy, sx);
    let k = info.illumination;
    let term_scale = 1.0 - 2.0 * k;
    let mut path: Vec<Point> = Vec::with_capacity(2 * (SAMPLES + 1));
    // Bright arc.
    for i in 0..=SAMPLES {
        let t = (i as f64) / (SAMPLES as f64);
        let theta = -std::f64::consts::FRAC_PI_2 + t * std::f64::consts::PI;
        let x = pos.x + r * (theta.cos() * sx + theta.sin() * tx);
        let y = pos.y + r * (theta.cos() * sy + theta.sin() * ty);
        path.push(Point::new(x, y));
    }
    // Terminator arc (reverse direction so the polygon is a single
    // closed loop sharing the horns).
    for i in 0..=SAMPLES {
        let t = (i as f64) / (SAMPLES as f64);
        let theta = std::f64::consts::FRAC_PI_2 - t * std::f64::consts::PI;
        let cx_local = r * term_scale * theta.cos();
        let cy_local = r * theta.sin();
        let x = pos.x + cx_local * sx + cy_local * tx;
        let y = pos.y + cx_local * sy + cy_local * ty;
        path.push(Point::new(x, y));
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// Polygon-area sanity: the path returned by `lit_region_path`
    /// must enclose ~`k`-fraction of the disc. Pins the sign of the
    /// `term_scale = 1 - 2k` formula — flipping it (the regression
    /// we just fixed) rendered gibbous moons as crescents and vice
    /// versa.
    #[test]
    fn polygon_area_matches_k() {
        let r = 100.0_f64;
        let centre = Point::new(0.0, 0.0);
        for &k in &[0.0_f64, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0] {
            let info = MoonPhaseInfo {
                illumination: k,
                sun_dir: (1.0, 0.0),
            };
            let path = lit_region_path(centre, r, info);
            // Shoelace formula for signed polygon area; abs() at the
            // end since winding direction depends on the SAMPLES
            // direction.
            let mut area2 = 0.0;
            for i in 0..path.len() {
                let a = path[i];
                let b = path[(i + 1) % path.len()];
                area2 += a.x * b.y - b.x * a.y;
            }
            let area = (area2 * 0.5).abs();
            let expected = k * PI * r * r;
            assert!(
                (area - expected).abs() < 0.005 * PI * r * r,
                "k={k}: enclosed area {area:.1} should match {expected:.1} (within 0.5% of disc)"
            );
        }
    }

    /// At gibbous phase (k > 0.5), the terminator apex must be on the
    /// **anti-Sun** side of the disc. This is the geometric fact we
    /// got wrong before: a positive `term_scale` put the apex on the
    /// Sun side, so the path enclosed a small lune instead of a
    /// large bulge.
    #[test]
    fn gibbous_terminator_bulges_into_dark_side() {
        let r = 100.0_f64;
        let centre = Point::new(0.0, 0.0);
        // Sun straight up (+y).
        let info = MoonPhaseInfo {
            illumination: 0.75,
            sun_dir: (0.0, 1.0),
        };
        let path = lit_region_path(centre, r, info);
        // The midpoint of the terminator arc (second half of the
        // path) sits at the apex. For 48 samples per arc, the
        // terminator midpoint is path[SAMPLES + 1 + 24] (after the
        // 49 bright-arc points). Index it loosely and look for
        // the minimum-y point on the second half.
        let mid = (path.len() / 2) + 24;
        let apex = path[mid.min(path.len() - 1)];
        // Sun is at +y, so anti-Sun is -y. Apex y should be
        // strongly negative.
        assert!(
            apex.y < -0.3 * r,
            "gibbous terminator apex should be on the anti-Sun side: got y={}",
            apex.y
        );
    }

    /// The user-visible bug: lit side appeared to rotate when the
    /// user moved the phone, even though the Sun-Moon geometry in
    /// the sky is fixed. Symptom of using the camera-frame 3-D chord
    /// vector instead of the perspective-projected screen direction.
    ///
    /// This test fixes the Sun + Moon at known equatorial coords,
    /// renders the camera at three different yaw angles, and asserts
    /// `sun_dir` always matches the real screen vector from
    /// projected-moon to projected-sun (both bodies in front of the
    /// camera in this setup). If a future refactor reintroduces the
    /// chord shortcut, the off-axis cases here will fail.
    #[test]
    fn sun_dir_matches_perspective_projection_at_multiple_yaws() {
        use crate::math::{
            equatorial_to_horizontal, horizontal_to_cartesian,
            HorizontalCoords,
        };
        use nalgebra::{UnitQuaternion, Vector3};

        // Equator observer at LST=0. Moon at alt=45° due north
        // (RA=0, dec=π/4); sun at alt=30°, az=30° east of north
        // (RA=0.713, dec=0.848). Both clearly in front of a camera
        // looking 45° up at any yaw within ±15°.
        let lat = 0.0_f64;
        let lst = 0.0_f64;
        let moon = EquatorialCoords { ra: 0.0, dec: std::f64::consts::FRAC_PI_4 };
        let sun = EquatorialCoords { ra: 0.713, dec: 0.848 };

        for &yaw_deg in &[-12.0_f64, 0.0, 12.0] {
            let yaw = yaw_deg.to_radians();
            let pitch = std::f64::consts::FRAC_PI_4; // 45° up — looks at the moon
            let q = UnitQuaternion::from_axis_angle(&Vector3::x_axis(), pitch)
                * UnitQuaternion::from_axis_angle(&Vector3::y_axis(), yaw);
            let rot = q.to_rotation_matrix().into_inner();

            let info = moon_phase_info(sun, moon, lat, lst, &rot);

            // Compute the "ground truth" — project both bodies and
            // take the screen-space delta. Same math as
            // SkyViewWidget::project_horizontal, inlined here so the
            // test stands on its own.
            let project = |coords: EquatorialCoords| -> Option<(f64, f64)> {
                let h = equatorial_to_horizontal(coords, lat, lst);
                let cart = horizontal_to_cartesian(h);
                let v = rot * cart;
                if v.z <= 0.05 {
                    return None;
                }
                Some((v.x / v.z, v.y / v.z))
            };
            let m_screen = project(moon).expect("moon in front of camera for this yaw");
            let s_screen = project(sun).expect("sun in front of camera for this yaw");
            let dx_true = s_screen.0 - m_screen.0;
            let dy_true = s_screen.1 - m_screen.1;
            let len_true = (dx_true * dx_true + dy_true * dy_true).sqrt();
            let true_dir = (dx_true / len_true, dy_true / len_true);

            let dot = info.sun_dir.0 * true_dir.0 + info.sun_dir.1 * true_dir.1;
            // dot ≈ 1 ↔ directions aligned. Anything less than ~0.999
            // means the visible lit limb is mis-rotated by several
            // degrees — the user can see that.
            assert!(
                dot > 0.999,
                "yaw={yaw_deg}°: sun_dir={:?} doesn't match screen direction {:?} (dot={dot:.4})",
                info.sun_dir,
                true_dir
            );
        }
    }

    /// Sun and Moon at the same ecliptic longitude (new moon) → 0%
    /// illumination. Sun and Moon opposed (full moon) → 100%.
    #[test]
    fn illumination_extremes() {
        let sun = EquatorialCoords { ra: 0.0, dec: 0.0 };
        let moon_new = EquatorialCoords { ra: 0.0, dec: 0.0 };
        let moon_full = EquatorialCoords { ra: PI, dec: 0.0 };
        let moon_quarter = EquatorialCoords {
            ra: PI / 2.0,
            dec: 0.0,
        };
        assert!(moon_illumination(sun, moon_new) < 1e-9);
        assert!((moon_illumination(sun, moon_full) - 1.0).abs() < 1e-9);
        assert!((moon_illumination(sun, moon_quarter) - 0.5).abs() < 1e-9);
    }
}
