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
use std::sync::OnceLock;

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

/// Extended catalog of named bright stars, parsed once from the bundled
/// CSV asset. IDs start at 100 to avoid collision with [`BRIGHTEST_STARS`]
/// (which the constellation-line table references by ID). Magnitudes
/// extend to roughly V≈4.4 so the sky reads as actually-populated under
/// dark conditions instead of the sparse 26-star seed set.
///
/// The eventual scope (per `implementation.md` §3.2) is the full Yale
/// Bright Star Catalog (~9k entries, ~150 KB compressed). This curated
/// ~160-star set is the intermediate step before we wire up that asset
/// pipeline.
const EXTENDED_CATALOG_CSV: &str = include_str!("../assets/bright_stars.csv");

/// Lazily-built combined view: seeded [`BRIGHTEST_STARS`] followed by the
/// parsed CSV catalog. Names from the CSV are heap-allocated once at
/// startup and leaked into the static lifetime so callers can keep using
/// `&'static str` (matching the seed table). The leak is bounded —
/// happens exactly once per process.
static ALL_STARS: OnceLock<Vec<Star>> = OnceLock::new();

/// Return every fixed star known to the renderer (seed + extended).
/// Sky-view rendering iterates this; constellation-line ID lookups stay
/// on [`BRIGHTEST_STARS`] since those IDs live in 1..=26 only.
pub fn all_stars() -> &'static [Star] {
    ALL_STARS.get_or_init(|| {
        let mut v: Vec<Star> = BRIGHTEST_STARS.to_vec();
        v.extend(parse_extended_catalog(EXTENDED_CATALOG_CSV));
        v
    })
}

/// Parse the CSV asset. Each line: `id,name,ra_rad,dec_rad,mag,bv`.
/// Malformed lines are skipped (logged in debug only) — the asset is
/// authored alongside the parser and a malformed row indicates a typo we
/// want to notice in development without crashing the app in production.
fn parse_extended_catalog(csv: &str) -> Vec<Star> {
    let mut out = Vec::with_capacity(256);
    for line in csv.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split(',');
        let Some(id) = parts.next().and_then(|s| s.trim().parse::<u32>().ok()) else {
            debug_assert!(false, "bright_stars.csv: bad id in {line:?}");
            continue;
        };
        let Some(name) = parts.next().map(|s| s.trim().to_string()) else {
            debug_assert!(false, "bright_stars.csv: missing name in {line:?}");
            continue;
        };
        let Some(ra) = parts.next().and_then(|s| s.trim().parse::<f64>().ok()) else {
            debug_assert!(false, "bright_stars.csv: bad ra in {line:?}");
            continue;
        };
        let Some(dec) = parts.next().and_then(|s| s.trim().parse::<f64>().ok()) else {
            debug_assert!(false, "bright_stars.csv: bad dec in {line:?}");
            continue;
        };
        let Some(mag) = parts.next().and_then(|s| s.trim().parse::<f32>().ok()) else {
            debug_assert!(false, "bright_stars.csv: bad mag in {line:?}");
            continue;
        };
        let Some(bv) = parts.next().and_then(|s| s.trim().parse::<f32>().ok()) else {
            debug_assert!(false, "bright_stars.csv: bad bv in {line:?}");
            continue;
        };
        out.push(Star {
            id,
            name: Box::leak(name.into_boxed_str()),
            coords: EquatorialCoords { ra, dec },
            magnitude: mag,
            color_index: bv,
        });
    }
    out
}

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

/// Approximate Keplerian + Meeus positions for the visible Solar System
/// bodies at `timestamp_ms` (Unix milliseconds, UTC). Outputs are J2000.0
/// equatorial coordinates in radians, suitable for piping straight into
/// [`crate::math::equatorial_to_horizontal`].
///
/// - **Sun**: textbook low-precision ecliptic formula (good to ~0.01°).
/// - **Moon**: Meeus truncated theory using the principal periodic terms
///   — well within the sub-degree budget called out in section 3.2 of
///   `implementation.md`.
/// - **Planets**: simplified ecliptic-circle approximation; the planet's
///   heliocentric position is computed from its mean longitude, then we
///   subtract Earth's heliocentric position and rotate into equatorial
///   coordinates by the obliquity. Visible naked-eye planets only
///   (Mercury, Venus, Mars, Jupiter, Saturn) — the user-stated use case
///   "Venus + Jupiter at sunset" hinges on this list.
pub fn calculate_solar_system_bodies(timestamp_ms: i64) -> Vec<CelestialBody> {
    let jd = crate::math::unix_to_julian_date(timestamp_ms);
    let d = jd - 2451545.0;
    let epsilon = (23.439 - 0.0000004 * d).to_radians();

    // ── Sun (ecliptic low-precision formula) ─────────────────────────────────
    let sun_l = wrap_360(280.460 + 0.9856474 * d);
    let sun_g = wrap_360(357.528 + 0.9856003 * d);
    let sun_lambda_deg = sun_l
        + 1.915 * sun_g.to_radians().sin()
        + 0.020 * (2.0 * sun_g).to_radians().sin();
    let sun_lambda = sun_lambda_deg.to_radians();
    let sun_coords = EquatorialCoords {
        ra: wrap_2pi((sun_lambda.sin() * epsilon.cos()).atan2(sun_lambda.cos())),
        dec: (epsilon.sin() * sun_lambda.sin()).asin(),
    };

    // ── Moon (Meeus low-order; principal periodic terms) ─────────────────────
    let moon_lp = wrap_360(218.316 + 13.176396 * d);
    let moon_m  = wrap_360(134.963 + 13.064993 * d);
    let moon_d  = wrap_360(297.850 + 12.190749 * d);
    let moon_f  = wrap_360( 93.272 + 13.229350 * d);
    let moon_lambda_deg = moon_lp
        + 6.289 * moon_m.to_radians().sin()
        + 1.274 * (2.0 * moon_d - moon_m).to_radians().sin()
        + 0.658 * (2.0 * moon_d).to_radians().sin();
    let moon_lambda = moon_lambda_deg.to_radians();
    let moon_beta = (5.128 * moon_f.to_radians().sin()).to_radians();
    let cos_beta = moon_beta.cos();
    let y = moon_lambda.sin() * cos_beta * epsilon.cos() - moon_beta.sin() * epsilon.sin();
    let x = moon_lambda.cos() * cos_beta;
    let moon_coords = EquatorialCoords {
        ra: wrap_2pi(y.atan2(x)),
        dec: (moon_lambda.sin() * cos_beta * epsilon.sin() + moon_beta.sin() * epsilon.cos())
            .asin(),
    };

    // ── Naked-eye planets (heliocentric → geocentric → equatorial) ───────────
    // Mean-longitude table (degrees + degrees/day) sourced from the NASA JPL
    // "Approximate Positions of the Planets" series, simplified to circular
    // orbits in the ecliptic plane. Inclination is folded in as a small
    // out-of-plane Z component.
    let earth = PlanetMeanOrbit {
        l_0: 100.464,
        l_dot: 0.985_600_3,
        a: 1.000,
        i_deg: 0.0,
    };
    let mercury = PlanetMeanOrbit {
        l_0: 252.250_906,
        l_dot: 4.092_338,
        a: 0.387,
        i_deg: 7.005,
    };
    let venus = PlanetMeanOrbit {
        l_0: 181.979_130,
        l_dot: 1.602_136,
        a: 0.723,
        i_deg: 3.395,
    };
    let mars = PlanetMeanOrbit {
        l_0: 355.453,
        l_dot: 0.524_020_8,
        a: 1.524,
        i_deg: 1.850,
    };
    let jupiter = PlanetMeanOrbit {
        l_0: 34.404,
        l_dot: 0.083_085_3,
        a: 5.203,
        i_deg: 1.305,
    };
    let saturn = PlanetMeanOrbit {
        l_0: 50.077_471,
        l_dot: 0.033_460,
        a: 9.537,
        i_deg: 2.485,
    };

    let earth_pos = earth.heliocentric_pos(d);

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
            name: "Mercury",
            coords: planet_eq_from_helio(mercury.heliocentric_pos(d), earth_pos, epsilon),
            magnitude: 0.0,
            color: Color::from_rgb8(200, 200, 200),
        },
        CelestialBody {
            name: "Venus",
            coords: planet_eq_from_helio(venus.heliocentric_pos(d), earth_pos, epsilon),
            magnitude: -4.4,
            color: Color::from_rgb8(255, 240, 200),
        },
        CelestialBody {
            name: "Mars",
            coords: planet_eq_from_helio(mars.heliocentric_pos(d), earth_pos, epsilon),
            magnitude: 1.5,
            color: Color::from_rgb8(230, 100, 80),
        },
        CelestialBody {
            name: "Jupiter",
            coords: planet_eq_from_helio(jupiter.heliocentric_pos(d), earth_pos, epsilon),
            magnitude: -2.0,
            color: Color::from_rgb8(240, 200, 160),
        },
        CelestialBody {
            name: "Saturn",
            coords: planet_eq_from_helio(saturn.heliocentric_pos(d), earth_pos, epsilon),
            magnitude: 0.6,
            color: Color::from_rgb8(220, 200, 150),
        },
    ]
}

/// Mean-orbit elements for a planet, simplified to circular + small
/// inclination. Enough for "where is Venus right now" naked-eye accuracy.
#[derive(Debug, Clone, Copy)]
struct PlanetMeanOrbit {
    /// Mean longitude at J2000.0 epoch (degrees).
    l_0: f64,
    /// Mean longitude rate of change (degrees per day).
    l_dot: f64,
    /// Semi-major axis (AU).
    a: f64,
    /// Orbital inclination (degrees).
    i_deg: f64,
}

impl PlanetMeanOrbit {
    /// Heliocentric ecliptic 3D position (AU) at `d` days past J2000.0.
    fn heliocentric_pos(&self, d: f64) -> [f64; 3] {
        let l_rad = wrap_360(self.l_0 + self.l_dot * d).to_radians();
        let i = self.i_deg.to_radians();
        let x = self.a * l_rad.cos();
        let y = self.a * l_rad.sin() * i.cos();
        let z = self.a * l_rad.sin() * i.sin();
        [x, y, z]
    }
}

/// Convert a planet's heliocentric ecliptic position to geocentric
/// equatorial (RA, Dec) coordinates.
fn planet_eq_from_helio(
    planet: [f64; 3],
    earth: [f64; 3],
    obliquity_rad: f64,
) -> EquatorialCoords {
    // Geocentric ecliptic position = planet - earth.
    let gx = planet[0] - earth[0];
    let gy = planet[1] - earth[1];
    let gz = planet[2] - earth[2];

    // Rotate from ecliptic to equatorial coordinates (rotation around the
    // ecliptic X axis by the obliquity).
    let cos_e = obliquity_rad.cos();
    let sin_e = obliquity_rad.sin();
    let eq_x = gx;
    let eq_y = gy * cos_e - gz * sin_e;
    let eq_z = gy * sin_e + gz * cos_e;

    let r_xy = (eq_x * eq_x + eq_y * eq_y).sqrt();
    EquatorialCoords {
        ra: wrap_2pi(eq_y.atan2(eq_x)),
        dec: eq_z.atan2(r_xy),
    }
}

/// Wrap a degree value to `[0, 360)`.
fn wrap_360(a: f64) -> f64 {
    let mut v = a % 360.0;
    if v < 0.0 {
        v += 360.0;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// Sanity-check that the Sun's RA at J2000.0 epoch (Jan 1, 2000 noon UT)
    /// lands near 18h22m, declination near -23° (it's a few weeks past the
    /// December solstice). Tolerance is generous to allow the
    /// low-precision formula's drift.
    #[test]
    fn sun_position_at_j2000() {
        // Unix ms for 2000-01-01T12:00:00Z = 946728000000
        let bodies = calculate_solar_system_bodies(946_728_000_000);
        let sun = bodies.iter().find(|b| b.name == "Sun").expect("Sun present");
        // RA expected ≈ 18.75h = 280.6°; allow ±5° slop for the truncated formula.
        let ra_deg = sun.coords.ra.to_degrees();
        let dec_deg = sun.coords.dec.to_degrees();
        assert!(
            (ra_deg - 280.6).abs() < 5.0,
            "Sun RA at J2000 should be near 280.6°, got {ra_deg:.2}°"
        );
        assert!(
            (dec_deg - (-23.0)).abs() < 3.0,
            "Sun Dec at J2000 should be near -23°, got {dec_deg:.2}°"
        );
    }

    /// All 7 named bodies must be present so the sky_view rendering doesn't
    /// silently lose Venus etc. if calculate_* gets refactored.
    #[test]
    fn all_named_bodies_emitted() {
        let bodies = calculate_solar_system_bodies(946_728_000_000);
        let names: Vec<&str> = bodies.iter().map(|b| b.name).collect();
        for expected in ["Sun", "Moon", "Mercury", "Venus", "Mars", "Jupiter", "Saturn"] {
            assert!(
                names.contains(&expected),
                "expected {expected} in {names:?}"
            );
        }
    }

    /// Sanity-check the parsed extended catalog: it must populate, all
    /// rows must parse (no silent skips), every star must have a unique
    /// ID, magnitudes/coordinates must be physically sensible, and the
    /// seeded constellation-line IDs (1..=26) must still resolve in
    /// `BRIGHTEST_STARS` so the asterism overlay can't quietly break.
    #[test]
    fn extended_catalog_parses_and_is_consistent() {
        let stars = all_stars();
        assert!(
            stars.len() > 100,
            "expected substantial extended catalog, got {} stars",
            stars.len()
        );
        // IDs must be unique across seed + extended set.
        let mut ids: Vec<u32> = stars.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate star IDs in combined catalog");
        // Constellation-line endpoints all live in 1..=26 — must still
        // be findable.
        for line in CONSTELLATION_LINES {
            assert!(
                BRIGHTEST_STARS.iter().any(|s| s.id == line.from_id),
                "missing from_id {} for {}",
                line.from_id,
                line.constellation_name
            );
            assert!(
                BRIGHTEST_STARS.iter().any(|s| s.id == line.to_id),
                "missing to_id {} for {}",
                line.to_id,
                line.constellation_name
            );
        }
        // Every star must have plausible coords + magnitude.
        for s in stars {
            assert!(
                s.coords.ra >= 0.0 && s.coords.ra < 2.0 * PI,
                "{} RA out of [0, 2π): {}",
                s.name,
                s.coords.ra
            );
            assert!(
                s.coords.dec >= -PI / 2.0 && s.coords.dec <= PI / 2.0,
                "{} Dec out of [-π/2, π/2]: {}",
                s.name,
                s.coords.dec
            );
            assert!(
                s.magnitude > -2.0 && s.magnitude < 8.0,
                "{} magnitude implausible: {}",
                s.name,
                s.magnitude
            );
        }
    }

    /// Coordinates should be normalized into the documented ranges.
    #[test]
    fn coordinates_in_expected_ranges() {
        let bodies = calculate_solar_system_bodies(946_728_000_000);
        for body in &bodies {
            assert!(
                body.coords.ra >= 0.0 && body.coords.ra < 2.0 * PI,
                "{} RA out of [0, 2π): {}",
                body.name,
                body.coords.ra
            );
            assert!(
                body.coords.dec >= -PI / 2.0 && body.coords.dec <= PI / 2.0,
                "{} Dec out of [-π/2, π/2]: {}",
                body.name,
                body.coords.dec
            );
        }
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
