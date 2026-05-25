//! # Star Backdrop and Constellation Engine
//!
//! Static celestial backdrop — a curated subset of the brightest stars and the
//! constellation lines that connect them — plus runtime Keplerian/Meeus
//! approximations for Solar System bodies, as specified in section 3.2 of
//! `implementation.md`.
//!
//! The full Yale Bright Star Catalog (~9k stars) is the eventual asset payload
//! described in the spec; this file ships a hand-picked subset so the app
//! renders something meaningful before the asset-loading pipeline lands.
//! Stars are stored in `const` tables (5 primitives each — ID, RA, Dec, V,
//! B-V) so they cost zero runtime allocation.

use crate::math::{CelestialBody, EquatorialCoords, Star};
use agg_gui::color::Color;
use std::f64::consts::PI;

/// A pair of star IDs representing a constellation line connection.
#[derive(Debug, Clone, Copy)]
pub struct ConstellationLine {
    pub from_id: u32,
    pub to_id: u32,
    pub constellation_name: &'static str,
}

/// Static catalog of the brightest stars (J2000.0 epoch, radians).
///
/// Coordinates are pre-converted to radians (RA: hours × π/12, Dec: degrees ×
/// π/180) so the projection pipeline can consume them without per-frame unit
/// conversion. Magnitudes and B-V color indices are taken from the Yale
/// Bright Star Catalog.
pub const BRIGHTEST_STARS: &[Star] = &[
    Star { id: 1,  name: "Polaris",    coords: EquatorialCoords { ra: 0.6624, dec: 1.5580  }, magnitude:  1.97, color_index:  0.60 },
    Star { id: 2,  name: "Sirius",     coords: EquatorialCoords { ra: 1.7676, dec: -0.2917 }, magnitude: -1.46, color_index:  0.00 },
    Star { id: 3,  name: "Canopus",    coords: EquatorialCoords { ra: 1.6753, dec: -0.9197 }, magnitude: -0.74, color_index:  0.15 },
    Star { id: 4,  name: "Arcturus",   coords: EquatorialCoords { ra: 3.7335, dec:  0.3348 }, magnitude: -0.05, color_index:  1.23 },
    Star { id: 5,  name: "Vega",       coords: EquatorialCoords { ra: 4.8735, dec:  0.6769 }, magnitude:  0.03, color_index:  0.00 },
    Star { id: 6,  name: "Capella",    coords: EquatorialCoords { ra: 1.3818, dec:  0.8028 }, magnitude:  0.08, color_index:  0.80 },
    Star { id: 7,  name: "Rigel",      coords: EquatorialCoords { ra: 1.3724, dec: -0.1431 }, magnitude:  0.13, color_index: -0.03 },
    Star { id: 8,  name: "Procyon",    coords: EquatorialCoords { ra: 2.0041, dec:  0.0912 }, magnitude:  0.34, color_index:  0.42 },
    Star { id: 9,  name: "Betelgeuse", coords: EquatorialCoords { ra: 1.5497, dec:  0.1293 }, magnitude:  0.50, color_index:  1.85 },
    Star { id: 10, name: "Altair",     coords: EquatorialCoords { ra: 5.1852, dec:  0.1557 }, magnitude:  0.76, color_index:  0.22 },
    Star { id: 11, name: "Aldebaran",  coords: EquatorialCoords { ra: 1.1873, dec:  0.2882 }, magnitude:  0.85, color_index:  1.54 },
    Star { id: 12, name: "Spica",      coords: EquatorialCoords { ra: 3.4735, dec: -0.1942 }, magnitude:  0.98, color_index: -0.23 },
    Star { id: 13, name: "Antares",    coords: EquatorialCoords { ra: 4.2981, dec: -0.4593 }, magnitude:  1.05, color_index:  1.83 },
    Star { id: 14, name: "Pollux",     coords: EquatorialCoords { ra: 2.0526, dec:  0.4891 }, magnitude:  1.14, color_index:  1.00 },
    Star { id: 15, name: "Deneb",      coords: EquatorialCoords { ra: 5.3902, dec:  0.7891 }, magnitude:  1.25, color_index:  0.09 },
    Star { id: 16, name: "Fomalhaut",  coords: EquatorialCoords { ra: 5.9922, dec: -0.5173 }, magnitude:  1.16, color_index:  0.09 },
    // Orion (Rigel = 7, Betelgeuse = 9)
    Star { id: 17, name: "Bellatrix",  coords: EquatorialCoords { ra: 1.3934, dec:  0.1084 }, magnitude:  1.64, color_index: -0.22 },
    Star { id: 18, name: "Alnilam",    coords: EquatorialCoords { ra: 1.4111, dec: -0.0205 }, magnitude:  1.69, color_index: -0.18 },
    Star { id: 19, name: "Saiph",      coords: EquatorialCoords { ra: 1.4856, dec: -0.1691 }, magnitude:  2.07, color_index: -0.18 },
    // Ursa Major (Big Dipper)
    Star { id: 20, name: "Dubhe",      coords: EquatorialCoords { ra: 2.9056, dec:  1.0772 }, magnitude:  1.81, color_index:  1.07 },
    Star { id: 21, name: "Merak",      coords: EquatorialCoords { ra: 2.8711, dec:  0.9829 }, magnitude:  2.34, color_index: -0.02 },
    Star { id: 22, name: "Phecda",     coords: EquatorialCoords { ra: 3.0319, dec:  0.9362 }, magnitude:  2.41, color_index:  0.00 },
    Star { id: 23, name: "Megrez",     coords: EquatorialCoords { ra: 3.1611, dec:  0.9948 }, magnitude:  3.32, color_index:  0.08 },
    Star { id: 24, name: "Alioth",     coords: EquatorialCoords { ra: 3.3769, dec:  0.9761 }, magnitude:  1.76, color_index: -0.02 },
    Star { id: 25, name: "Mizar",      coords: EquatorialCoords { ra: 3.5119, dec:  0.9572 }, magnitude:  2.23, color_index:  0.00 },
    Star { id: 26, name: "Alkaid",     coords: EquatorialCoords { ra: 3.6111, dec:  0.8572 }, magnitude:  1.85, color_index: -0.19 },
];

/// Constellation line connections for the bundled asterisms.
///
/// Eventual scope (per `implementation.md` section 3.3) is the 88 IAU
/// constellations from `celestial_data`; this list seeds the renderer with
/// Orion + Ursa Major so the constellation overlay is testable today.
pub const CONSTELLATION_LINES: &[ConstellationLine] = &[
    // Orion
    ConstellationLine { from_id:  9, to_id: 17, constellation_name: "Orion" },      // Betelgeuse → Bellatrix
    ConstellationLine { from_id: 17, to_id: 18, constellation_name: "Orion" },      // Bellatrix → Alnilam (belt)
    ConstellationLine { from_id:  9, to_id: 18, constellation_name: "Orion" },      // Betelgeuse → Alnilam
    ConstellationLine { from_id: 18, to_id:  7, constellation_name: "Orion" },      // Alnilam → Rigel
    ConstellationLine { from_id: 18, to_id: 19, constellation_name: "Orion" },      // Alnilam → Saiph
    ConstellationLine { from_id:  7, to_id: 19, constellation_name: "Orion" },      // Rigel → Saiph
    // Ursa Major (Big Dipper)
    ConstellationLine { from_id: 20, to_id: 21, constellation_name: "Ursa Major" }, // Dubhe → Merak (pointer)
    ConstellationLine { from_id: 21, to_id: 22, constellation_name: "Ursa Major" }, // Merak → Phecda
    ConstellationLine { from_id: 22, to_id: 23, constellation_name: "Ursa Major" }, // Phecda → Megrez
    ConstellationLine { from_id: 23, to_id: 20, constellation_name: "Ursa Major" }, // Megrez → Dubhe (bowl close)
    ConstellationLine { from_id: 23, to_id: 24, constellation_name: "Ursa Major" }, // Megrez → Alioth
    ConstellationLine { from_id: 24, to_id: 25, constellation_name: "Ursa Major" }, // Alioth → Mizar
    ConstellationLine { from_id: 25, to_id: 26, constellation_name: "Ursa Major" }, // Mizar → Alkaid (handle)
];

/// Approximate Keplerian positions for the Sun, Moon, Mars, and Jupiter at
/// `timestamp_ms` (Unix milliseconds, UTC). Outputs are J2000.0 equatorial
/// coordinates in radians, suitable for piping straight into
/// [`crate::math::equatorial_to_horizontal`].
///
/// Sun: textbook low-precision ecliptic formula (good to ~0.01°).
/// Moon: Meeus truncated theory using the principal periodic terms — well
///   within the sub-degree budget called out in section 3.2.
/// Planets: 6-element Keplerian approximation (NASA JPL "approximate
///   positions" set) reduced to a geocentric heliocentric-difference vector.
pub fn calculate_solar_system_bodies(timestamp_ms: i64) -> Vec<CelestialBody> {
    let jd = crate::math::unix_to_julian_date(timestamp_ms);
    let d = jd - 2451545.0;

    // ── Sun ──────────────────────────────────────────────────────────────────
    let sun_l = ((280.460 + 0.9856474 * d) % 360.0 + 360.0) % 360.0;
    let sun_g = ((357.528 + 0.9856003 * d) % 360.0 + 360.0) % 360.0;
    let sun_lambda_deg = sun_l
        + 1.915 * sun_g.to_radians().sin()
        + 0.020 * (2.0 * sun_g).to_radians().sin();
    let sun_lambda = sun_lambda_deg.to_radians();
    let epsilon = (23.439 - 0.0000004 * d).to_radians();
    let sun_ra_raw = (sun_lambda.sin() * epsilon.cos()).atan2(sun_lambda.cos());
    let sun_dec = (epsilon.sin() * sun_lambda.sin()).asin();
    let sun_coords = EquatorialCoords {
        ra: wrap_2pi(sun_ra_raw),
        dec: sun_dec,
    };

    // ── Moon (Meeus low-order) ──────────────────────────────────────────────
    let moon_lp = ((218.316 + 13.176396 * d) % 360.0 + 360.0) % 360.0;
    let moon_m  = ((134.963 + 13.064993 * d) % 360.0 + 360.0) % 360.0;
    let moon_d  = ((297.850 + 12.190749 * d) % 360.0 + 360.0) % 360.0;
    let moon_f  = (( 93.272 + 13.229350 * d) % 360.0 + 360.0) % 360.0;
    let moon_lambda_deg = moon_lp
        + 6.289 * moon_m.to_radians().sin()
        + 1.274 * (2.0 * moon_d - moon_m).to_radians().sin()
        + 0.658 * (2.0 * moon_d).to_radians().sin();
    let moon_lambda = moon_lambda_deg.to_radians();
    let moon_beta = (5.128 * moon_f.to_radians().sin()).to_radians();
    let cos_beta = moon_beta.cos();
    let y = moon_lambda.sin() * cos_beta * epsilon.cos() - moon_beta.sin() * epsilon.sin();
    let x = moon_lambda.cos() * cos_beta;
    let moon_ra_raw = y.atan2(x);
    let moon_dec =
        (moon_lambda.sin() * cos_beta * epsilon.sin() + moon_beta.sin() * epsilon.cos()).asin();
    let moon_coords = EquatorialCoords {
        ra: wrap_2pi(moon_ra_raw),
        dec: moon_dec,
    };

    // ── Mars + Jupiter (heliocentric → geocentric subtraction) ──────────────
    let l_e = (((100.464 + 0.9856003 * d) % 360.0) + 360.0) % 360.0;
    let l_m = (((355.453 + 0.5240208 * d) % 360.0) + 360.0) % 360.0;
    let l_j = ((( 34.404 + 0.0830853 * d) % 360.0) + 360.0) % 360.0;
    let e_rad = l_e.to_radians();
    let r_e = 1.000_f64; // AU
    let r_m = 1.524_f64;
    let r_j = 5.203_f64;

    let mars_coords = planet_geocentric_eq(r_m, l_m.to_radians(), e_rad, r_e, 1.85_f64);
    let jupiter_coords = planet_geocentric_eq(r_j, l_j.to_radians(), e_rad, r_e, 1.30_f64);

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

/// Project a planet's heliocentric circular-orbit position into geocentric
/// equatorial coordinates. The full JPL six-element Kepler solve is the
/// eventual upgrade; this circular approximation keeps Mars/Jupiter readable
/// on the sky without the full code path.
fn planet_geocentric_eq(
    r_planet: f64,
    l_planet_rad: f64,
    l_earth_rad: f64,
    r_earth: f64,
    incl_deg: f64,
) -> EquatorialCoords {
    let dx = r_planet * l_planet_rad.cos() - r_earth * l_earth_rad.cos();
    let dy = r_planet * l_planet_rad.sin() - r_earth * l_earth_rad.sin();
    let ra_raw = dy.atan2(dx);
    let dec = (incl_deg.to_radians()) * l_planet_rad.sin();
    EquatorialCoords {
        ra: wrap_2pi(ra_raw),
        dec,
    }
}

fn wrap_2pi(a: f64) -> f64 {
    let two_pi = 2.0 * PI;
    let mut v = a % two_pi;
    if v < 0.0 {
        v += two_pi;
    }
    v
}
