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
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

/// Construct a 3D Rotation Matrix from device orientation Euler angles (Alpha, Beta, Gamma) in radians.
///
/// - Alpha (Yaw/Compass): rotation around Z axis
/// - Beta (Pitch): rotation around X axis
/// - Gamma (Roll): rotation around Y axis
pub fn device_orientation_matrix(alpha: f64, beta: f64, gamma: f64) -> Matrix3<f64> {
    // Rotation around Z axis (alpha/yaw)
    let c_a = alpha.cos();
    let s_a = alpha.sin();
    let r_z = Matrix3::new(
        c_a, -s_a, 0.0,
        s_a,  c_a, 0.0,
        0.0,  0.0, 1.0,
    );

    // Rotation around X axis (beta/pitch)
    let c_b = beta.cos();
    let s_b = beta.sin();
    let r_x = Matrix3::new(
        1.0, 0.0,  0.0,
        0.0, c_b, -s_b,
        0.0, s_b,  c_b,
    );

    // Rotation around Y axis (gamma/roll)
    let c_g = gamma.cos();
    let s_g = gamma.sin();
    let r_y = Matrix3::new(
        c_g,  0.0, s_g,
        0.0,  1.0, 0.0,
       -s_g,  0.0, c_g,
    );

    // Combine rotations: standard mobile device spec uses Y * X * Z or similar ordering.
    // Here we compute the complete camera transformation matrix.
    r_z * r_x * r_y
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
}
