//! # Astronomical and Projection Mathematics
//!
//! This module provides the mathematical computations required for Instant-Astronomer,
//! including Local Sidereal Time (LST), coordinate transformations from Equatorial
//! (RA/Dec) to Horizontal (Alt/Az) coordinates, 3D Cartesian projection, device
//! telemetry integration, and a low-pass filter for smoothing sensor jitter.
//!
//! It is used by the sky rendering widget to map star and planetary coordinates
//! dynamically relative to the user's geographical position, current time, and device orientation.

use nalgebra::{Matrix3, Vector3};
use std::f64::consts::PI;

/// Coordinates in the equatorial system: Right Ascension and Declination.
#[derive(Debug, Clone, Copy)]
pub struct EquatorialCoords {
    /// Right Ascension in radians (0 to 2*PI, corresponding to 0h to 24h)
    pub ra: f64,
    /// Declination in radians (-PI/2 to PI/2, corresponding to -90 to +90 degrees)
    pub dec: f64,
}

/// Coordinates in the horizontal system: Altitude and Azimuth.
#[derive(Debug, Clone, Copy)]
pub struct HorizontalCoords {
    /// Altitude in radians (-PI/2 to PI/2). Angle above the horizon.
    pub alt: f64,
    /// Azimuth in radians (0 to 2*PI). Angle clockwise from North.
    pub az: f64,
}

/// Representation of a fixed star backdrop element.
///
/// `name` is `&'static str` so the catalog (section 3.2 of
/// `implementation.md` — Yale Bright Star Catalog primitives) can live in a
/// `const` table without runtime allocation.
#[derive(Debug, Clone, Copy)]
pub struct Star {
    pub id: u32,
    pub name: &'static str,
    pub coords: EquatorialCoords,
    /// Visual magnitude (smaller is brighter)
    pub magnitude: f32,
    /// B-V color index (used for star color classification)
    pub color_index: f32,
}

/// Representation of a dynamic solar system body (Sun, Moon, Planets).
#[derive(Debug, Clone)]
pub struct CelestialBody {
    pub name: &'static str,
    pub coords: EquatorialCoords,
    pub magnitude: f32,
    pub color: agg_gui::Color,
}

/// Low-pass filter for smoothing high-frequency magnetometer / gyroscope jitter.
/// Follows the telemetry smoothing specification:
/// theta_filtered = theta_filtered + kappa * (theta_raw - theta_filtered)
#[derive(Debug, Clone)]
pub struct LowPassFilter {
    alpha: f64, // Jitter reduction coefficient (kappa, default 0.12)
    filtered_yaw: f64,
    filtered_pitch: f64,
    filtered_roll: f64,
    initialized: bool,
}

impl LowPassFilter {
    /// Create a new lowpass telemetry smoothing filter with a given kappa coefficient.
    pub fn new(kappa: f64) -> Self {
        Self {
            alpha: kappa.clamp(0.0, 1.0),
            filtered_yaw: 0.0,
            filtered_pitch: 0.0,
            filtered_roll: 0.0,
            initialized: false,
        }
    }

    /// Feed a raw (yaw, pitch, roll) orientation reading in radians and retrieve the smoothed state.
    /// Handles wrap-around correctly (since angles wrap at 2*PI).
    pub fn update(&mut self, yaw: f64, pitch: f64, roll: f64) -> (f64, f64, f64) {
        if !self.initialized {
            self.filtered_yaw = yaw;
            self.filtered_pitch = pitch;
            self.filtered_roll = roll;
            self.initialized = true;
            return (yaw, pitch, roll);
        }

        // Helper to interpolate angles with wrap-around
        let lerp_angle = |from: f64, to: f64, alpha: f64| -> f64 {
            let mut diff = to - from;
            // Normalize diff to -PI to PI
            while diff < -PI {
                diff += 2.0 * PI;
            }
            while diff > PI {
                diff -= 2.0 * PI;
            }
            from + alpha * diff
        };

        self.filtered_yaw = lerp_angle(self.filtered_yaw, yaw, self.alpha);
        self.filtered_pitch = lerp_angle(self.filtered_pitch, pitch, self.alpha);
        self.filtered_roll = lerp_angle(self.filtered_roll, roll, self.alpha);

        (self.filtered_yaw, self.filtered_pitch, self.filtered_roll)
    }
}

/// Compute the Julian Date from a Unix timestamp in milliseconds.
pub fn unix_to_julian_date(timestamp_ms: i64) -> f64 {
    let unix_epoch_julian = 2440587.5;
    let ms_per_day = 86_400_000.0;
    unix_epoch_julian + (timestamp_ms as f64 / ms_per_day)
}

/// Compute the Local Sidereal Time (LST) in radians.
///
/// LST represents the angle of the vernal equinox relative to the local observer's meridian.
/// Formulas adapted from Meeus:
/// Greenwich Mean Sidereal Time (GMST) at 0h UT can be approximated,
/// and combined with the observer's longitude and time elapsed.
pub fn compute_local_sidereal_time(timestamp_ms: i64, longitude_rad: f64) -> f64 {
    let jd = unix_to_julian_date(timestamp_ms);
    let t = (jd - 2451545.0) / 36525.0;

    // Greenwich Mean Sidereal Time (GMST) in degrees
    let mut gmst = 280.46061837
        + 360.98564736629 * (jd - 2451545.0)
        + t * t * (0.000387933 - t / 38_710_000.0);

    // Normalize GMST to 0 to 360 degrees
    gmst = gmst % 360.0;
    if gmst < 0.0 {
        gmst += 360.0;
    }

    let gmst_rad = gmst.to_radians();

    // Local Sidereal Time = GMST + Longitude
    let mut lst = gmst_rad + longitude_rad;

    // Normalize LST to 0 to 2*PI radians
    while lst < 0.0 {
        lst += 2.0 * PI;
    }
    while lst >= 2.0 * PI {
        lst -= 2.0 * PI;
    }

    lst
}

/// Geometric horizon altitude (radians) used by [`rise_set_times`] for a
/// "standard" star — i.e. atmospheric refraction lifts an object roughly
/// 34' above the geometric horizon at rise/set, so we report the time it
/// reaches apparent altitude -34' rather than 0°.
pub const STANDARD_REFRACTION_ALT_RAD: f64 = -0.009_890_2; // -0.566° in rad

/// Horizon altitude for the Sun's *upper limb*: -0.5667° refraction
/// minus 0.2667° apparent radius = -0.8333°.
pub const SUN_HORIZON_ALT_RAD: f64 = -0.014_543_4; // -0.833° in rad

/// Result of a rise/set lookup. `Times` carries Unix epoch ms for the
/// rise/set events bracketing `around_ts_ms`; the other two variants
/// describe a body that's currently in a circumpolar state at the
/// observer's latitude.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RiseSet {
    /// The body crosses the horizon during the 24h window around the
    /// query time. Both timestamps are in Unix epoch ms.
    Times { rise_ms: i64, set_ms: i64 },
    /// The body is above the horizon all day at this latitude.
    AlwaysUp,
    /// The body never rises at this latitude today.
    NeverRises,
}

/// Approximate rise / set times for a body at (`coords.ra`, `coords.dec`),
/// observed from (`latitude_rad`, `longitude_rad`, east positive), near
/// the time given by `around_ts_ms`. `horizon_alt_rad` is the apparent
/// altitude at which the event is reported — pass
/// [`STANDARD_REFRACTION_ALT_RAD`] for stars / planets and
/// [`SUN_HORIZON_ALT_RAD`] for the Sun's upper limb.
///
/// This is a single-shot calculation that assumes the body's equatorial
/// coordinates are constant across the 24h window. That's exact for
/// stars, fine for planets (sub-degree drift), and good to a few
/// minutes for the Moon (whose RA / Dec moves ~13°/day). For naked-
/// eye stargazing accuracy the latter is good enough.
pub fn rise_set_times(
    coords: EquatorialCoords,
    latitude_rad: f64,
    longitude_rad: f64,
    around_ts_ms: i64,
    horizon_alt_rad: f64,
) -> RiseSet {
    let cos_h0 = (horizon_alt_rad.sin() - latitude_rad.sin() * coords.dec.sin())
        / (latitude_rad.cos() * coords.dec.cos());
    if cos_h0 > 1.0 {
        return RiseSet::NeverRises;
    }
    if cos_h0 < -1.0 {
        return RiseSet::AlwaysUp;
    }
    let h0 = cos_h0.acos();

    // LST values when the body is at the rise / set horizon.
    let two_pi = 2.0 * PI;
    let lst_rise = ((coords.ra - h0) % two_pi + two_pi) % two_pi;
    let lst_set = ((coords.ra + h0) % two_pi + two_pi) % two_pi;

    let rise_ms = unix_ms_for_lst(lst_rise, longitude_rad, around_ts_ms);
    let set_ms = unix_ms_for_lst(lst_set, longitude_rad, around_ts_ms);
    RiseSet::Times { rise_ms, set_ms }
}

/// Format a [`RiseSet`] for display in the user's local time. Uses
/// the platform-reported UTC offset in minutes (DST applied) so the
/// times read as wall-clock at the observer's location, not UTC.
/// 12-hour AM/PM format. Examples:
/// - `"Rises 6:42pm · Sets 6:13am"`
/// - `"Always up"` / `"Below horizon today"`
pub fn format_rise_set(rs: RiseSet, offset_minutes: i32) -> String {
    match rs {
        RiseSet::AlwaysUp => String::from("Always up"),
        RiseSet::NeverRises => String::from("Below horizon today"),
        RiseSet::Times { rise_ms, set_ms } => format!(
            "Rises {} · Sets {}",
            format_hhmm(rise_ms, offset_minutes),
            format_hhmm(set_ms, offset_minutes),
        ),
    }
}

/// Format a Unix-ms timestamp as 12-hour `H:MMam`/`H:MMpm` in the
/// observer's local (offset-adjusted) wall clock. Wraps cleanly
/// across day boundaries. Used by the rise/set reticle card so the
/// times read the way a North-American user expects (`11:23pm`)
/// instead of the 24-hour `23:23`.
pub fn format_hhmm(unix_ms: i64, offset_minutes: i32) -> String {
    let local_ms = unix_ms + (offset_minutes as i64) * 60_000;
    let h24 = ((local_ms / 3_600_000) % 24 + 24) % 24;
    let m = ((local_ms / 60_000) % 60 + 60) % 60;
    let (h12, ampm) = match h24 {
        0 => (12, "am"),
        1..=11 => (h24, "am"),
        12 => (12, "pm"),
        _ => (h24 - 12, "pm"),
    };
    format!("{h12}:{m:02}{ampm}")
}

/// Invert [`compute_local_sidereal_time`] linearly: find the Unix ms
/// closest to `around_ts_ms` for which the local sidereal time equals
/// `target_lst_rad`. Pure linear approximation — GMST advances at
/// ~360.98565° per UT day, so the higher-order T² term in the full
/// IAU formula contributes <1 ms across any 24h window and is dropped
/// here for clarity.
fn unix_ms_for_lst(target_lst_rad: f64, longitude_rad: f64, around_ts_ms: i64) -> i64 {
    let now_lst_rad = compute_local_sidereal_time(around_ts_ms, longitude_rad);
    let mut delta = target_lst_rad - now_lst_rad;
    // Wrap to [-π, π] so we pick the nearest occurrence (within ±12h).
    while delta > PI {
        delta -= 2.0 * PI;
    }
    while delta < -PI {
        delta += 2.0 * PI;
    }
    // Sidereal day = 86_164.0905 s ≈ 23h56m4.0905s. So a 2π gain in
    // LST takes one sidereal day; per-radian it's 86_164 / (2π) s.
    let seconds_per_rad: f64 = 86_164.0905 / (2.0 * PI);
    around_ts_ms + (delta * seconds_per_rad * 1000.0) as i64
}

/// Transform Equatorial coordinates (RA/Dec) to Horizontal coordinates (Alt/Az).
///
/// Under the following equations:
/// sin(Alt) = sin(Dec)sin(Lat) + cos(Dec)cos(Lat)cos(HourAngle)
/// cos(Az)  = (sin(Dec) - sin(Alt)sin(Lat)) / (cos(Alt)cos(Lat))
///
/// HourAngle (H) = Local Sidereal Time (LST) - Right Ascension (RA)
pub fn equatorial_to_horizontal(
    coords: EquatorialCoords,
    latitude_rad: f64,
    lst_rad: f64,
) -> HorizontalCoords {
    let dec = coords.dec;
    let ra = coords.ra;

    // Hour Angle
    let h = lst_rad - ra;

    // Altitude (Alt)
    let sin_alt = dec.sin() * latitude_rad.sin() + dec.cos() * latitude_rad.cos() * h.cos();
    let alt = sin_alt.asin();

    // Azimuth (Az)
    // Avoid division by zero if Alt is exactly at the zenith (+90) or nadir (-90)
    let az = if alt.cos().abs() < 1e-6 {
        0.0 // Azimuth is undefined at zenith/nadir; default to North
    } else {
        let cos_az = (dec.sin() - alt.sin() * latitude_rad.sin()) / (alt.cos() * latitude_rad.cos());
        // Clamp to avoid float precision issues outside [-1, 1]
        let cos_az_clamped = cos_az.clamp(-1.0, 1.0);
        let mut az_calc = cos_az_clamped.acos();

        // Adjust azimuth based on Hour Angle
        if h.sin() > 0.0 {
            az_calc = 2.0 * PI - az_calc;
        }
        az_calc
    };

    HorizontalCoords { alt, az }
}

/// Project Horizontal coordinates (Alt/Az) on a unit sphere (r = 1.0) into
/// 3D Cartesian coordinates (X, Y, Z).
///
/// In our Y-up coordinate system:
/// - Z-axis points North (Azimuth = 0, Altitude = 0)
/// - X-axis points East (Azimuth = PI/2, Altitude = 0)
/// - Y-axis points Zenith (Altitude = PI/2, straight up)
pub fn horizontal_to_cartesian(coords: HorizontalCoords) -> Vector3<f64> {
    let r = 1.0;
    let alt = coords.alt;
    let az = coords.az;

    // Y is Altitude up (Zenith)
    let y = r * alt.sin();

    // Horizontal component on the ground plane (X-Z)
    let ground_r = r * alt.cos();

    // Azimuth: 0 is North (+Z), PI/2 is East (+X)
    let z = ground_r * az.cos();
    let x = ground_r * az.sin();

    Vector3::new(x, y, z)
}

/// Construct the world→camera rotation matrix used by the sky-view
/// projection.
///
/// Our world frame is **Y-up, Z-north, X-east** (Y is the zenith axis).
/// So the angles plug in as follows:
///
/// - `yaw` (compass / `alpha`, in W3C CCW-from-north convention): rotation
///   around **Y** (the zenith). +yaw = the camera turns counter-clockwise
///   when viewed from above (i.e. swings toward west).
/// - `pitch` (look up / down): rotation around **X** (east). +pitch = the
///   camera tilts up toward zenith.
/// - `roll` (camera bank): rotation around **Z** (north). Currently the
///   sky-view passes 0 in for roll because tracking the user's phone
///   roll just makes the horizon line jitter; the math is ready for it
///   if we want to wire it back in.
///
/// Composition order: `Rx(pitch) * Ry(yaw)`. Apply yaw first (around the
/// world zenith) so a horizontal pan sweeps the compass; then pitch
/// (around the world east axis). This is the standard FPS-camera
/// "yaw-then-pitch" Tait-Bryan YXZ that doesn't roll the horizon when
/// the user simply turns in place.
///
/// **Note**: an earlier version of this function rotated yaw around our
/// Z axis (which is *north*, not up) — that put a "roll" into what was
/// labelled "yaw" and was the cause of the compass tape not sliding
/// correctly when the user spun in place.
pub fn device_orientation_matrix(yaw: f64, pitch: f64, _roll: f64) -> Matrix3<f64> {
    // Yaw around the world up axis (+Y / zenith).
    let c_y = yaw.cos();
    let s_y = yaw.sin();
    let r_y = Matrix3::new(
        c_y,  0.0, s_y,
        0.0,  1.0, 0.0,
       -s_y,  0.0, c_y,
    );

    // Pitch around the world east axis (+X).
    let c_p = pitch.cos();
    let s_p = pitch.sin();
    let r_x = Matrix3::new(
        1.0, 0.0,  0.0,
        0.0, c_p, -s_p,
        0.0, s_p,  c_p,
    );

    // World → camera: first the yaw (around world up), then pitch
    // (around world east). Matrix product is reversed: Rx * Ry.
    r_x * r_y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lst() {
        let timestamp_ms = 1779836400000; // Sometime in 2026
        let longitude = -104.9903 * PI / 180.0; // Denver
        let lst = compute_local_sidereal_time(timestamp_ms, longitude);
        assert!(lst >= 0.0 && lst < 2.0 * PI);
    }

    #[test]
    fn test_coordinate_conversion() {
        let dec = 45.0f64.to_radians();
        let ra = 10.0f64.to_radians();
        let lat = 40.0f64.to_radians();
        let lst = 12.0f64.to_radians();

        let eq = EquatorialCoords { ra, dec };
        let horiz = equatorial_to_horizontal(eq, lat, lst);

        assert!(horiz.alt >= -PI/2.0 && horiz.alt <= PI/2.0);
        assert!(horiz.az >= 0.0 && horiz.az <= 2.0 * PI);
    }

    /// Regression test for the yaw-axis bug. Before the fix,
    /// `device_orientation_matrix` rotated yaw around the world Z axis
    /// (which in our frame is *north*, not up) so what the code called
    /// "yaw" was actually a roll. Now yaw correctly rotates around the
    /// zenith (Y).
    ///
    /// With yaw=90° CCW (= facing west) and pitch=0:
    /// - North (0,0,1) should swing toward camera-east  -> +X side of
    ///   the rotated frame.
    /// - Zenith (0,1,0) should stay at +Y (yaw doesn't tilt the head).
    #[test]
    fn yaw_rotates_around_zenith_not_north() {
        let r = device_orientation_matrix((PI / 2.0).into(), 0.0, 0.0);
        let north = nalgebra::Vector3::new(0.0, 0.0, 1.0);
        let zenith = nalgebra::Vector3::new(0.0, 1.0, 0.0);
        let n_rot = r * north;
        let z_rot = r * zenith;

        // North should rotate into +X (camera-right) when facing west.
        assert!((n_rot.x - 1.0).abs() < 1e-9, "n_rot.x = {}", n_rot.x);
        assert!(n_rot.y.abs() < 1e-9, "n_rot.y = {}", n_rot.y);
        assert!(n_rot.z.abs() < 1e-9, "n_rot.z = {}", n_rot.z);

        // Zenith must not move when only yaw changes.
        assert!((z_rot.x).abs() < 1e-9, "z_rot.x = {}", z_rot.x);
        assert!((z_rot.y - 1.0).abs() < 1e-9, "z_rot.y = {}", z_rot.y);
        assert!((z_rot.z).abs() < 1e-9, "z_rot.z = {}", z_rot.z);
    }

    /// Pitch rotates around the world east axis (+X). Tilting up (pitch
    /// = +90°) brings the zenith to the camera-forward direction (+Z).
    #[test]
    fn pitch_brings_zenith_into_view_when_looking_up() {
        let r = device_orientation_matrix(0.0, (PI / 2.0).into(), 0.0);
        let zenith = nalgebra::Vector3::new(0.0, 1.0, 0.0);
        let z_rot = r * zenith;
        assert!(z_rot.x.abs() < 1e-9, "z_rot.x = {}", z_rot.x);
        assert!(z_rot.y.abs() < 1e-9, "z_rot.y = {}", z_rot.y);
        assert!((z_rot.z - 1.0).abs() < 1e-9, "z_rot.z = {}", z_rot.z);
    }

    /// End-to-end check that the alt=0 great circle projects to a
    /// strictly horizontal line on screen after any sequence of drags
    /// composed via SkyView's drag handler. This is the
    /// "ground plane stays level with the horizon" property the user
    /// can see: sample the alt=0 ring, project every visible point,
    /// assert all screen Ys agree.
    ///
    /// The previous `q_world_yaw * view_quat * q_local_pitch` form
    /// looked roll-free in algebra but accumulated small roll under
    /// diagonal drags (the camera-right axis tilted out of the world
    /// XZ plane). The fix replays each drag through a yaw/pitch
    /// decomposition — this test mirrors that exact composition.
    #[test]
    fn alt_zero_projects_to_horizontal_line_after_drags() {
        use nalgebra::{UnitQuaternion, Vector3};
        // Decompose-recompose drag composition — same rule the
        // SkyView mouse handler uses. Any roll that creeps in via
        // float drift is discarded by the (yaw, pitch) round-trip.
        let apply_drag = |q: UnitQuaternion<f64>, dx: f64, dy: f64| -> UnitQuaternion<f64> {
            let fwd = q.inverse_transform_vector(&Vector3::new(0.0, 0.0, 1.0));
            let cur_pitch = fwd.y.clamp(-1.0, 1.0).asin();
            let cur_yaw = (-fwd.x).atan2(fwd.z);
            let cap = PI / 2.0 - 0.01;
            let new_yaw = cur_yaw + (-dx);
            let new_pitch = (cur_pitch + dy).clamp(-cap, cap);
            let q_yaw = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), new_yaw);
            let q_pitch = UnitQuaternion::from_axis_angle(&Vector3::x_axis(), new_pitch);
            q_pitch * q_yaw
        };
        let mut q = UnitQuaternion::<f64>::identity();
        // Drags that previously induced roll under the world-yaw /
        // camera-pitch composition.
        q = apply_drag(q, 0.5, 0.4);
        q = apply_drag(q, -0.3, 0.2);
        q = apply_drag(q, 0.2, -0.1);
        q = apply_drag(q, 0.6, 0.3);

        // (a) Geometric: camera-right (camera +X) in world frame must
        // have zero world-Y component — that's literally "no roll."
        // `inverse_transform_vector` is the correct way to express
        // a view-frame vector in world coordinates (NOT `q * X`,
        // which transforms a world vector into view — the previous
        // test got this backwards and silently passed).
        let cam_right_in_world =
            q.inverse_transform_vector(&nalgebra::Vector3::x());
        assert!(
            cam_right_in_world.y.abs() < 1e-12,
            "camera right has non-zero world-Y component: {}",
            cam_right_in_world.y
        );

        // (b) Pixel-level: sample the alt=0 ring and project each
        // sample exactly like `paint_alt_zero_line` does. All visible
        // samples must land on the same screen Y.
        let rot = q.to_rotation_matrix().into_inner();
        let center_y = 300.0_f64;
        let focal = 500.0_f64;
        let mut shared_screen_y: Option<f64> = None;
        let mut samples_in_front = 0;
        let step = (2.0_f64).to_radians();
        let mut az = 0.0_f64;
        while az < 2.0 * PI {
            let hc = HorizontalCoords { alt: 0.0, az };
            let v_cart = horizontal_to_cartesian(hc);
            let v_rot = rot * v_cart;
            if v_rot.z > 0.02 {
                samples_in_front += 1;
                let sy = center_y + (v_rot.y / v_rot.z) * focal;
                match shared_screen_y {
                    None => shared_screen_y = Some(sy),
                    Some(prev) => assert!(
                        (sy - prev).abs() < 1e-6,
                        "alt=0 not horizontal at az={}: y={} vs {}",
                        az.to_degrees(),
                        sy,
                        prev
                    ),
                }
            }
            az += step;
        }
        assert!(samples_in_front > 0, "expected some alt=0 samples to project");
    }

    /// 12-hour AM/PM conversion. Pin down the four corner cases
    /// (midnight, noon, AM, PM) so a future refactor that swaps
    /// the format back to 24h has to update this test.
    #[test]
    fn format_hhmm_renders_12_hour_ampm() {
        // 0 offset, pick exact Unix ms at well-known UTC hours.
        // 2025-01-01T00:00:00Z = 1735689600000 ms
        let midnight = 1_735_689_600_000_i64;
        assert_eq!(format_hhmm(midnight, 0), "12:00am");
        assert_eq!(format_hhmm(midnight + 1 * 3_600_000, 0), "1:00am");
        assert_eq!(format_hhmm(midnight + 11 * 3_600_000, 0), "11:00am");
        assert_eq!(format_hhmm(midnight + 12 * 3_600_000, 0), "12:00pm");
        assert_eq!(format_hhmm(midnight + 13 * 3_600_000, 0), "1:00pm");
        assert_eq!(
            format_hhmm(midnight + 23 * 3_600_000 + 23 * 60_000, 0),
            "11:23pm"
        );
        // Offset shifts: PST = -480 min. 03:00 UTC + (-480 min) = 19:00
        // previous day → 7:00pm.
        assert_eq!(format_hhmm(midnight + 3 * 3_600_000, -480), "7:00pm");
    }

    /// On the equator with the body on the meridian *right now*, the
    /// rise and set events should bracket the current time symmetrically
    /// — roughly 6h before and after now, giving a 12h above-horizon
    /// window.
    #[test]
    fn rise_set_equator_yields_12h_window() {
        let now = 1_767_268_800_000_i64; // 2026-01-01T12:00:00Z
        let lat = 0.0;
        let lng = 0.0;
        let lst_now = compute_local_sidereal_time(now, lng);
        let coords = EquatorialCoords { ra: lst_now, dec: 0.0 };
        match rise_set_times(coords, lat, lng, now, STANDARD_REFRACTION_ALT_RAD) {
            RiseSet::Times { rise_ms, set_ms } => {
                let span_h = (set_ms - rise_ms) as f64 / 3_600_000.0;
                assert!(
                    (span_h - 12.0).abs() < 0.05,
                    "expected ~12h span at the equator, got {span_h:.3}h"
                );
                assert!(rise_ms < now, "rise should be in the past");
                assert!(set_ms > now, "set should be in the future");
            }
            other => panic!("expected Times, got {other:?}"),
        }
    }

    /// At a high northern latitude, a body at high northern declination
    /// is circumpolar; a body at deep southern declination never rises.
    /// Pins down the cos(H₀) branch logic.
    #[test]
    fn rise_set_handles_circumpolar_states() {
        let lat = (70.0_f64).to_radians();
        let north = EquatorialCoords {
            ra: 0.0,
            dec: (85.0_f64).to_radians(),
        };
        assert_eq!(
            rise_set_times(north, lat, 0.0, 0, STANDARD_REFRACTION_ALT_RAD),
            RiseSet::AlwaysUp
        );
        let south = EquatorialCoords {
            ra: 0.0,
            dec: (-85.0_f64).to_radians(),
        };
        assert_eq!(
            rise_set_times(south, lat, 0.0, 0, STANDARD_REFRACTION_ALT_RAD),
            RiseSet::NeverRises
        );
    }

    /// Sanity check that incremental quaternion composition produces
    /// the same result as a single equivalent rotation. Pins down the
    /// camera-local rotation pattern the SkyView mouse-drag handler
    /// uses, so a future refactor that swaps composition order can't
    /// silently break panning.
    #[test]
    fn camera_local_rotation_composes_cleanly() {
        use nalgebra::{UnitQuaternion, Vector3};
        let step = (5.0_f64).to_radians();
        // Eighteen 5° camera-local yaw steps == 90° total yaw.
        let mut q = UnitQuaternion::<f64>::identity();
        for _ in 0..18 {
            let delta = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), step);
            q = delta * q;
        }
        let direct = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), 18.0 * step);
        // Compare on the +Z (forward) axis after rotation.
        let v_incremental = q * Vector3::new(0.0, 0.0, 1.0);
        let v_direct = direct * Vector3::new(0.0, 0.0, 1.0);
        let dot = v_incremental.dot(&v_direct);
        assert!((dot - 1.0).abs() < 1e-9, "vectors not parallel: dot={}", dot);
    }
}
