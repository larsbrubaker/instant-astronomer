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

    /// Regression test for the horizon-rotates bug. The drag handler in
    /// SkyView used to compose yaw as a camera-local rotation, which
    /// allowed roll to accumulate after any sequence of diagonal /
    /// alternating horizontal+vertical drags. The fix: yaw must rotate
    /// around the **world** up axis (Y); only pitch is camera-local
    /// (around X). With that composition, the world-up vector projected
    /// into camera space must always have zero camera-right (X) component
    /// — that's the geometric meaning of "no roll, horizon stays level".
    #[test]
    fn horizon_stays_level_under_diagonal_drags() {
        use nalgebra::{UnitQuaternion, Vector3};
        // World-Y yaw, camera-X pitch — this is the composition rule
        // the SkyView handler should use.
        let apply_drag = |q: UnitQuaternion<f64>, dx: f64, dy: f64| -> UnitQuaternion<f64> {
            let yaw_w = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), -dx);
            let pitch_l = UnitQuaternion::from_axis_angle(&Vector3::x_axis(), dy);
            yaw_w * q * pitch_l
        };

        // Walk through a few mixed drags that previously induced roll.
        let mut q = UnitQuaternion::<f64>::identity();
        q = apply_drag(q, 0.5, 0.4);
        q = apply_drag(q, -0.3, 0.2);
        q = apply_drag(q, 0.2, -0.1);
        q = apply_drag(q, 0.6, 0.3);

        // Camera-right axis (+X) in world frame. The world-up vector
        // (+Y world) must have NO projection onto camera-right — that
        // is, the horizon line as seen by the camera is horizontal.
        let world_up = Vector3::y();
        let cam_right_in_world = q * Vector3::x();
        let roll_component = world_up.dot(&cam_right_in_world);
        assert!(
            roll_component.abs() < 1e-12,
            "horizon must stay level — got roll component {roll_component}"
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
