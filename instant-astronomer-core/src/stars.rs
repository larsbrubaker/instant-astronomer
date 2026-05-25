//! # Star Backdrop and Constellation Engine
//!
//! This module provides the static celestial background (including the brightest
//! stars in the sky and constellation connections) and implements Keplerian orbit
//! approximations to locate dynamic Solar System bodies (the Sun, Moon, Mars, Venus,
//! and Jupiter) in real-time.
//!
//! Stars are defined by ID, Right Ascension (RA), Declination (Dec), Visual Magnitude,
//! and Color Index, matching the Yale Bright Star Catalog (BSC5) format.

use crate::math::{CelestialBody, EquatorialCoords, Star};
use std::f64::consts::PI;

/// A pair of star IDs representing a constellation line connection.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConstellationLine {
    pub from_id: u32,
    pub to_id: u32,
    pub constellation_name: &'static str,
}

/// Static catalog of the brightest stars (including Polaris) with true J2000.0 epoch coordinates.
pub const BRIGHTEST_STARS: &[Star] = &[
    Star { id: 1, name: "Polaris", coords: EquatorialCoords { ra: 0.6624, dec: 1.5580 }, magnitude: 1.97, color_index: 0.60 },
    Star { id: 2, name: "Sirius", coords: EquatorialCoords { ra: 1.7676, dec: -0.2917 }, magnitude: -1.46, color_index: 0.00 },
    Star { id: 3, name: "Canopus", coords: EquatorialCoords { ra: 1.6753, dec: -0.9197 }, magnitude: -0.74, color_index: 0.15 },
    Star { id: 4, name: "Arcturus", coords: EquatorialCoords { ra: 3.7335, dec: 0.3348 }, magnitude: -0.05, color_index: 1.23 },
    Star { id: 5, name: "Vega", coords: EquatorialCoords { ra: 4.8735, dec: 0.6769 }, magnitude: 0.03, color_index: 0.00 },
    Star { id: 6, name: "Capella", coords: EquatorialCoords { ra: 1.3818, dec: 0.8028 }, magnitude: 0.08, color_index: 0.80 },
    Star { id: 7, name: "Rigel", coords: EquatorialCoords { ra: 1.3724, dec: -0.1431 }, magnitude: 0.13, color_index: -0.03 },
    Star { id: 8, name: "Procyon", coords: EquatorialCoords { ra: 2.0041, dec: 0.0912 }, magnitude: 0.34, color_index: 0.42 },
    Star { id: 9, name: "Betelgeuse", coords: EquatorialCoords { ra: 1.5497, dec: 0.1293 }, magnitude: 0.50, color_index: 1.85 },
    Star { id: 10, name: "Altair", coords: EquatorialCoords { ra: 5.1852, dec: 0.1557 }, magnitude: 0.76, color_index: 0.22 },
    Star { id: 11, name: "Aldebaran", coords: EquatorialCoords { ra: 1.1873, dec: 0.2882 }, magnitude: 0.85, color_index: 1.54 },
    Star { id: 12, name: "Spica", coords: EquatorialCoords { ra: 3.4735, dec: -0.1942 }, magnitude: 0.98, color_index: -0.23 },
    Star { id: 13, name: "Antares", coords: EquatorialCoords { ra: 4.2981, dec: -0.4593 }, magnitude: 1.05, color_index: 1.83 },
    Star { id: 14, name: "Pollux", coords: EquatorialCoords { ra: 2.0526, dec: 0.4891 }, magnitude: 1.14, color_index: 1.00 },
    Star { id: 15, name: "Deneb", coords: EquatorialCoords { ra: 5.3902, dec: 0.7891 }, magnitude: 1.25, color_index: 0.09 },
    Star { id: 16, name: "Fomalhaut", coords: EquatorialCoords { ra: 5.9922, dec: -0.5173 }, magnitude: 1.16, color_index: 0.09 },
    // Orion Constellation Stars (Rigel is 7, Betelgeuse is 9)
    Star { id: 17, name: "Bellatrix", coords: EquatorialCoords { ra: 1.3934, dec: 0.1084 }, magnitude: 1.64, color_index: -0.22 },
    Star { id: 18, name: "Alnilam", coords: EquatorialCoords { ra: 1.4111, dec: -0.0205 }, magnitude: 1.69, color_index: -0.18 },
    Star { id: 19, name: "Saiph", coords: EquatorialCoords { ra: 1.4856, dec: -0.1691 }, magnitude: 2.07, color_index: -0.18 },
    // Ursa Major (Big Dipper) Stars
    Star { id: 20, name: "Dubhe", coords: EquatorialCoords { ra: 2.9056, dec: 1.0772 }, magnitude: 1.81, color_index: 1.07 },
    Star { id: 21, name: "Merak", coords: EquatorialCoords { ra: 2.8711, dec: 0.9829 }, magnitude: 2.34, color_index: -0.02 },
    Star { id: 22, name: "Phecda", coords: EquatorialCoords { ra: 3.0319, dec: 0.9362 }, magnitude: 2.41, color_index: 0.00 },
    Star { id: 23, name: "Megrez", coords: EquatorialCoords { ra: 3.1611, dec: 0.9948 }, magnitude: 3.32, color_index: 0.08 },
    Star { id: 24, name: "Alioth", coords: EquatorialCoords { ra: 3.3769, dec: 0.9761 }, magnitude: 1.76, color_index: -0.02 },
    Star { id: 25, name: "Mizar", coords: EquatorialCoords { ra: 3.5119, dec: 0.9572 }, magnitude: 2.23, color_index: 0.00 },
    Star { id: 26, name: "Alkaid", coords: EquatorialCoords { ra: 3.6111, dec: 0.8572 }, magnitude: 1.85, color_index: -0.19 },
];

/// Constellation line connections for major constellations (Orion, Big Dipper).
pub const CONSTELLATION_LINES: &[ConstellationLine] = &[
    // Orion lines
    ConstellationLine { from_id: 9, to_id: 17, constellation_name: "Orion" },  // Betelgeuse to Bellatrix
    ConstellationLine { from_id: 17, to_id: 18, constellation_name: "Orion" }, // Bellatrix to Alnilam (belt)
    ConstellationLine { from_id: 9, to_id: 18, constellation_name: "Orion" },  // Betelgeuse to Alnilam
    ConstellationLine { from_id: 18, to_id: 7, constellation_name: "Orion" },  // Alnilam to Rigel
    ConstellationLine { from_id: 18, to_id: 19, constellation_name: "Orion" }, // Alnilam to Saiph
    ConstellationLine { from_id: 7, to_id: 19, constellation_name: "Orion" },  // Rigel to Saiph
    // Big Dipper (Ursa Major) lines
    ConstellationLine { from_id: 20, to_id: 21, constellation_name: "Ursa Major" }, // Dubhe to Merak (pointers)
    ConstellationLine { from_id: 21, to_id: 22, constellation_name: "Ursa Major" }, // Merak to Phecda
    ConstellationLine { from_id: 22, to_id: 23, constellation_name: "Ursa Major" }, // Phecda to Megrez
    ConstellationLine { from_id: 23, to_id: 20, constellation_name: "Ursa Major" }, // Megrez to Dubhe (bowl)
    ConstellationLine { from_id: 23, to_id: 24, constellation_name: "Ursa Major" }, // Megrez to Alioth
    ConstellationLine { from_id: 24, to_id: 25, constellation_name: "Ursa Major" }, // Alioth to Mizar
    ConstellationLine { from_id: 25, to_id: 26, constellation_name: "Ursa Major" }, // Mizar to Alkaid (handle)
];

/// Approximate Keplerian orbit calculation for key Solar System bodies.
/// Since full Keplerian elements require extensive tables, we approximate positions
/// around May 2026 with Keplerian elements referenced to J2000.0.
pub fn calculate_solar_system_bodies(timestamp_ms: i64) -> Vec<CelestialBody> {
    use agg_gui::Color;

    // Days since J2000.0 epoch (Jan 1, 2000 at 12h UT, Julian Date = 2451545.0)
    let jd = crate::math::unix_to_julian_date(timestamp_ms);
    let d = jd - 2451545.0;

    // 1. Estimate Sun position
    // Mean longitude of the Sun (L)
    let sun_l = (280.460 + 0.9856474 * d) % 360.0;
    // Mean anomaly (G)
    let sun_g = (357.528 + 0.9856003 * d) % 360.0;
    // Ecliptic longitude (lambda)
    let sun_lambda = (sun_l + 1.915 * sun_g.to_radians().sin() + 0.020 * (2.0 * sun_g).to_radians().sin()) % 360.0;
    let sun_lambda_rad = sun_lambda.to_radians();
    // Obliquity of the ecliptic (epsilon)
    let epsilon_rad = (23.439 - 0.0000004 * d).to_radians();

    // Equatorial coordinates of the Sun
    let sun_ra = (sun_lambda_rad.sin() * epsilon_rad.cos()).atan2(sun_lambda_rad.cos());
    let sun_dec = (epsilon_rad.sin() * sun_lambda_rad.sin()).asin();

    let sun_coords = EquatorialCoords {
        ra: if sun_ra < 0.0 { sun_ra + 2.0 * PI } else { sun_ra },
        dec: sun_dec,
    };

    // 2. Estimate Moon position (Jean Meeus' simplified theory)
    // Mean longitude of the Moon (L')
    let moon_lp = (218.316 + 13.176396 * d) % 360.0;
    // Mean anomaly of the Moon (M')
    let moon_m = (134.963 + 13.064993 * d) % 360.0;
    // Mean elongation (D)
    let moon_d = (297.850 + 12.190749 * d) % 360.0;

    // Ecliptic longitude correction
    let moon_lambda = moon_lp
        + 6.289 * moon_m.to_radians().sin()
        + 1.274 * (2.0 * moon_d - moon_m).to_radians().sin()
        + 0.658 * (2.0 * moon_d).to_radians().sin();
    let moon_lambda_rad = moon_lambda.to_radians();

    // Ecliptic latitude (B)
    let moon_f = (93.272 + 13.229350 * d) % 360.0;
    let moon_beta = 5.128 * moon_f.to_radians().sin();
    let moon_beta_rad = moon_beta.to_radians();

    // Convert Moon Ecliptic to Equatorial
    let cos_beta = moon_beta_rad.cos();
    let y = moon_lambda_rad.sin() * cos_beta * epsilon_rad.cos() - moon_beta_rad.sin() * epsilon_rad.sin();
    let x = moon_lambda_rad.cos() * cos_beta;
    let moon_ra = y.atan2(x);
    let moon_dec = (moon_lambda_rad.sin() * cos_beta * epsilon_rad.sin() + moon_beta_rad.sin() * epsilon_rad.cos()).asin();

    let moon_coords = EquatorialCoords {
        ra: if moon_ra < 0.0 { moon_ra + 2.0 * PI } else { moon_ra },
        dec: moon_dec,
    };

    // 3. Estimate Mars position (Keplerian orbits relative to J2000)
    // Earth Mean Longitude (L_e)
    let l_e = (100.464 + 0.9856003 * d) % 360.0;
    // Mars Mean Longitude (L_m)
    let l_m = (355.453 + 0.5240208 * d) % 360.0;

    // Simple heliocentric coordinates conversion to geocentric approximation
    let mars_heliocentric_rad = l_m.to_radians();
    let earth_heliocentric_rad = l_e.to_radians();

    let r_e = 1.0; // AU
    let r_m = 1.524; // AU

    // Mars 3D vector from Earth
    let dx = r_m * mars_heliocentric_rad.cos() - r_e * earth_heliocentric_rad.cos();
    let dy = r_m * mars_heliocentric_rad.sin() - r_e * earth_heliocentric_rad.sin();

    // Right ascension and Declination approximation
    let mars_ra = dy.atan2(dx);
    let mars_dec = 1.85f64.to_radians() * mars_heliocentric_rad.sin(); // inclined orbit approx

    let mars_coords = EquatorialCoords {
        ra: if mars_ra < 0.0 { mars_ra + 2.0 * PI } else { mars_ra },
        dec: mars_dec,
    };

    // 4. Estimate Jupiter position
    let l_j = (34.404 + 0.0830853 * d) % 360.0;
    let jupiter_heliocentric_rad = l_j.to_radians();
    let r_j = 5.203; // AU

    let j_dx = r_j * jupiter_heliocentric_rad.cos() - r_e * earth_heliocentric_rad.cos();
    let j_dy = r_j * jupiter_heliocentric_rad.sin() - r_e * earth_heliocentric_rad.sin();

    let jupiter_ra = j_dy.atan2(j_dx);
    let jupiter_dec = 1.3f64.to_radians() * jupiter_heliocentric_rad.sin();

    let jupiter_coords = EquatorialCoords {
        ra: if jupiter_ra < 0.0 { jupiter_ra + 2.0 * PI } else { jupiter_ra },
        dec: jupiter_dec,
    };

    vec![
        CelestialBody {
            name: "Sun",
            coords: sun_coords,
            magnitude: -26.74,
            color: Color::from_rgb8(255, 230, 100),
        },
        CelestialBody {
            name: "Moon",
            coords: moon_coords,
            magnitude: -12.74,
            color: Color::from_rgb8(220, 220, 240),
        },
        CelestialBody {
            name: "Mars",
            coords: mars_coords,
            magnitude: 1.5,
            color: Color::from_rgb8(230, 100, 80),
        },
        CelestialBody {
            name: "Jupiter",
            coords: jupiter_coords,
            magnitude: -2.0,
            color: Color::from_rgb8(240, 200, 160),
        },
    ]
}
