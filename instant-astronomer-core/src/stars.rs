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

/// A human-made spacecraft tracked as a fixed-position search target.
///
/// The Voyager probes are receding at the edge of the Solar System, so
/// their apparent position drifts only a few arcminutes per *year* as seen
/// from Earth — far below the pointing accuracy of a phone finder. We
/// therefore snapshot an apparent RA/Dec rather than running an ephemeris,
/// matching how named stars are stored (`SearchKind::Fixed`).
#[derive(Debug, Clone, Copy)]
pub struct Spacecraft {
    pub name: &'static str,
    pub coords: EquatorialCoords,
    /// Constellation the probe currently lies in front of (display only).
    pub constellation: &'static str,
}

/// Interstellar spacecraft we can point a finder at. Coordinates are
/// apparent RA/Dec (radians) snapshotted from TheSkyLive ephemerides for
/// mid-2026; see the per-entry comment for the source values.
pub const SPACECRAFT: &[Spacecraft] = &[
    // 17h 16m 47s, +12° 23′ 38″ (Ophiuchus) — 2026-06-19.
    Spacecraft {
        name: "Voyager 1",
        coords: EquatorialCoords { ra: 4.52382, dec: 0.21631 },
        constellation: "Ophiuchus",
    },
    // 20h 16m 16s, −59° 37′ 28″ (Pavo) — 2026-06-19.
    Spacecraft {
        name: "Voyager 2",
        coords: EquatorialCoords { ra: 5.30696, dec: -1.04065 },
        constellation: "Pavo",
    },
];

/// Constellation line connections for the bundled asterisms.
///
/// Eventual scope (per `implementation.md` section 3.3) is the 88 IAU
/// constellations from `celestial_data`; this list seeds the renderer with
/// Orion + Ursa Major so the constellation overlay is testable today.
pub const CONSTELLATION_LINES: &[ConstellationLine] = &[
    // ── Orion ──────────────────────────────────────────────────────────────
    ConstellationLine { from_id:  9, to_id: 17, constellation_name: "Orion" },          // Betelgeuse → Bellatrix
    ConstellationLine { from_id: 17, to_id: 18, constellation_name: "Orion" },          // Bellatrix → Alnilam (belt)
    ConstellationLine { from_id:  9, to_id: 18, constellation_name: "Orion" },          // Betelgeuse → Alnilam
    ConstellationLine { from_id: 18, to_id:  7, constellation_name: "Orion" },          // Alnilam → Rigel
    ConstellationLine { from_id: 18, to_id: 19, constellation_name: "Orion" },          // Alnilam → Saiph
    ConstellationLine { from_id:  7, to_id: 19, constellation_name: "Orion" },          // Rigel → Saiph
    // ── Ursa Major (Big Dipper) ────────────────────────────────────────────
    ConstellationLine { from_id: 20, to_id: 21, constellation_name: "Ursa Major" },     // Dubhe → Merak
    ConstellationLine { from_id: 21, to_id: 22, constellation_name: "Ursa Major" },     // Merak → Phecda
    ConstellationLine { from_id: 22, to_id: 23, constellation_name: "Ursa Major" },     // Phecda → Megrez
    ConstellationLine { from_id: 23, to_id: 20, constellation_name: "Ursa Major" },     // Megrez → Dubhe (bowl close)
    ConstellationLine { from_id: 23, to_id: 24, constellation_name: "Ursa Major" },     // Megrez → Alioth
    ConstellationLine { from_id: 24, to_id: 25, constellation_name: "Ursa Major" },     // Alioth → Mizar
    ConstellationLine { from_id: 25, to_id: 26, constellation_name: "Ursa Major" },     // Mizar → Alkaid (handle)
    // ── Ursa Minor (Little Dipper) ─────────────────────────────────────────
    ConstellationLine { from_id:   1, to_id: 102, constellation_name: "Ursa Minor" },   // Polaris → Yildun
    ConstellationLine { from_id: 102, to_id: 100, constellation_name: "Ursa Minor" },   // Yildun → Kochab
    ConstellationLine { from_id: 100, to_id: 101, constellation_name: "Ursa Minor" },   // Kochab → Pherkad
    // ── Cassiopeia (W) ─────────────────────────────────────────────────────
    ConstellationLine { from_id: 104, to_id: 103, constellation_name: "Cassiopeia" },   // Caph → Schedar
    ConstellationLine { from_id: 103, to_id: 105, constellation_name: "Cassiopeia" },   // Schedar → Gamma Cas
    ConstellationLine { from_id: 105, to_id: 106, constellation_name: "Cassiopeia" },   // Gamma Cas → Ruchbah
    ConstellationLine { from_id: 106, to_id: 107, constellation_name: "Cassiopeia" },   // Ruchbah → Segin
    // ── Cepheus ────────────────────────────────────────────────────────────
    ConstellationLine { from_id: 186, to_id: 187, constellation_name: "Cepheus" },      // Alderamin → Alfirk
    ConstellationLine { from_id: 187, to_id: 188, constellation_name: "Cepheus" },      // Alfirk → Errai
    // ── Draco (winding) ────────────────────────────────────────────────────
    ConstellationLine { from_id: 181, to_id: 182, constellation_name: "Draco" },        // Eltanin → Rastaban
    ConstellationLine { from_id: 182, to_id: 183, constellation_name: "Draco" },        // Rastaban → Altais
    ConstellationLine { from_id: 183, to_id: 185, constellation_name: "Draco" },        // Altais → Edasich
    ConstellationLine { from_id: 185, to_id: 184, constellation_name: "Draco" },        // Edasich → Thuban
    // ── Cygnus (Northern Cross) ────────────────────────────────────────────
    ConstellationLine { from_id:  15, to_id: 191, constellation_name: "Cygnus" },       // Deneb → Sadr (spine)
    ConstellationLine { from_id: 191, to_id: 193, constellation_name: "Cygnus" },       // Sadr → Albireo (head)
    ConstellationLine { from_id: 191, to_id: 192, constellation_name: "Cygnus" },       // Sadr → Gienah (wing)
    ConstellationLine { from_id: 191, to_id: 194, constellation_name: "Cygnus" },       // Sadr → Aljanah (wing)
    // ── Lyra ───────────────────────────────────────────────────────────────
    ConstellationLine { from_id:   5, to_id: 189, constellation_name: "Lyra" },         // Vega → Sheliak
    ConstellationLine { from_id: 189, to_id: 190, constellation_name: "Lyra" },         // Sheliak → Sulafat
    ConstellationLine { from_id: 190, to_id:   5, constellation_name: "Lyra" },         // Sulafat → Vega (close triangle)
    // ── Aquila ─────────────────────────────────────────────────────────────
    ConstellationLine { from_id:  10, to_id: 195, constellation_name: "Aquila" },       // Altair → Tarazed
    ConstellationLine { from_id:  10, to_id: 196, constellation_name: "Aquila" },       // Altair → Alshain
    // ── Bootes (kite) ──────────────────────────────────────────────────────
    ConstellationLine { from_id:   4, to_id: 167, constellation_name: "Bootes" },       // Arcturus → Muphrid
    ConstellationLine { from_id:   4, to_id: 166, constellation_name: "Bootes" },       // Arcturus → Izar
    ConstellationLine { from_id: 166, to_id: 169, constellation_name: "Bootes" },       // Izar → Nekkar
    ConstellationLine { from_id: 169, to_id: 168, constellation_name: "Bootes" },       // Nekkar → Seginus
    ConstellationLine { from_id: 168, to_id: 167, constellation_name: "Bootes" },       // Seginus → Muphrid
    // ── Corona Borealis ────────────────────────────────────────────────────
    ConstellationLine { from_id: 170, to_id: 171, constellation_name: "Corona Borealis" }, // Alphecca → Nusakan
    // ── Hercules (rough keystone hint) ─────────────────────────────────────
    ConstellationLine { from_id: 172, to_id: 173, constellation_name: "Hercules" },     // Kornephoros → Rasalgethi
    ConstellationLine { from_id: 172, to_id: 174, constellation_name: "Hercules" },     // Kornephoros → Sarin
    // ── Andromeda ──────────────────────────────────────────────────────────
    ConstellationLine { from_id: 108, to_id: 109, constellation_name: "Andromeda" },    // Alpheratz → Mirach
    ConstellationLine { from_id: 109, to_id: 110, constellation_name: "Andromeda" },    // Mirach → Almach
    // ── Pegasus (Great Square + Enif) ──────────────────────────────────────
    ConstellationLine { from_id: 108, to_id: 113, constellation_name: "Pegasus" },      // Alpheratz → Algenib
    ConstellationLine { from_id: 113, to_id: 111, constellation_name: "Pegasus" },      // Algenib → Markab
    ConstellationLine { from_id: 111, to_id: 112, constellation_name: "Pegasus" },      // Markab → Scheat
    ConstellationLine { from_id: 112, to_id: 108, constellation_name: "Pegasus" },      // Scheat → Alpheratz (close square)
    ConstellationLine { from_id: 111, to_id: 114, constellation_name: "Pegasus" },      // Markab → Enif (nose)
    // ── Perseus ────────────────────────────────────────────────────────────
    ConstellationLine { from_id: 116, to_id: 117, constellation_name: "Perseus" },      // Mirfak → Algol
    ConstellationLine { from_id: 116, to_id: 118, constellation_name: "Perseus" },      // Mirfak → Atik
    // ── Auriga (pentagon) ──────────────────────────────────────────────────
    ConstellationLine { from_id:   6, to_id: 119, constellation_name: "Auriga" },       // Capella → Menkalinan
    ConstellationLine { from_id: 119, to_id: 143, constellation_name: "Auriga" },       // Menkalinan → Elnath (shared w/ Taurus)
    ConstellationLine { from_id: 143, to_id: 120, constellation_name: "Auriga" },       // Elnath → Mahasim
    ConstellationLine { from_id: 120, to_id: 121, constellation_name: "Auriga" },       // Mahasim → Almaaz
    ConstellationLine { from_id: 121, to_id:   6, constellation_name: "Auriga" },       // Almaaz → Capella
    // ── Canis Major (Sirius leads, then Wezen + Adhara triangle) ───────────
    ConstellationLine { from_id: 130, to_id:   2, constellation_name: "Canis Major" },  // Mirzam → Sirius
    ConstellationLine { from_id:   2, to_id: 128, constellation_name: "Canis Major" },  // Sirius → Adhara
    ConstellationLine { from_id: 128, to_id: 129, constellation_name: "Canis Major" },  // Adhara → Wezen
    ConstellationLine { from_id: 129, to_id: 131, constellation_name: "Canis Major" },  // Wezen → Aludra
    ConstellationLine { from_id: 128, to_id: 132, constellation_name: "Canis Major" },  // Adhara → Furud
    // ── Canis Minor ────────────────────────────────────────────────────────
    ConstellationLine { from_id:   8, to_id: 133, constellation_name: "Canis Minor" },  // Procyon → Gomeisa
    // ── Lepus (the Hare) ───────────────────────────────────────────────────
    ConstellationLine { from_id: 137, to_id: 138, constellation_name: "Lepus" },        // Arneb → Nihal
    // ── Cetus ──────────────────────────────────────────────────────────────
    ConstellationLine { from_id: 149, to_id: 152, constellation_name: "Cetus" },        // Diphda → Baten Kaitos
    ConstellationLine { from_id: 152, to_id: 151, constellation_name: "Cetus" },        // Baten Kaitos → Kaffaljidhma
    ConstellationLine { from_id: 151, to_id: 150, constellation_name: "Cetus" },        // Kaffaljidhma → Menkar
    // ── Crux (Southern Cross) ──────────────────────────────────────────────
    ConstellationLine { from_id: 240, to_id: 242, constellation_name: "Crux" },         // Acrux → Gacrux
    ConstellationLine { from_id: 241, to_id: 243, constellation_name: "Crux" },         // Mimosa → Imai
    // ── Centaurus (α/β missing from catalog; minimal asterism) ─────────────
    ConstellationLine { from_id: 237, to_id: 239, constellation_name: "Centaurus" },    // Hadar → Muhlifain
    ConstellationLine { from_id: 239, to_id: 238, constellation_name: "Centaurus" },    // Muhlifain → Menkent
    // ── Ophiuchus (13th sun-transit; not part of tropical zodiac) ──────────
    ConstellationLine { from_id: 175, to_id: 176, constellation_name: "Ophiuchus" },    // Rasalhague → Cebalrai
    ConstellationLine { from_id: 175, to_id: 178, constellation_name: "Ophiuchus" },    // Rasalhague → Yed Prior
    ConstellationLine { from_id: 178, to_id: 179, constellation_name: "Ophiuchus" },    // Yed Prior → Yed Posterior
    ConstellationLine { from_id: 179, to_id: 177, constellation_name: "Ophiuchus" },    // Yed Posterior → Sabik
    ConstellationLine { from_id: 177, to_id: 176, constellation_name: "Ophiuchus" },    // Sabik → Cebalrai

    // ═════════════════════════════════════════════════════════════════════
    // Zodiac constellations — see `zodiac_date_range` for tropical dates.
    // ═════════════════════════════════════════════════════════════════════
    // ── Aries ──────────────────────────────────────────────────────────────
    ConstellationLine { from_id: 153, to_id: 154, constellation_name: "Aries" },        // Hamal → Sheratan
    ConstellationLine { from_id: 154, to_id: 155, constellation_name: "Aries" },        // Sheratan → Mesarthim
    // ── Taurus (simplified: horn + Pleiades direction) ─────────────────────
    ConstellationLine { from_id: 143, to_id:  11, constellation_name: "Taurus" },       // Elnath → Aldebaran
    ConstellationLine { from_id:  11, to_id: 144, constellation_name: "Taurus" },       // Aldebaran → Alcyone (Pleiades)
    // ── Gemini (twins) ─────────────────────────────────────────────────────
    ConstellationLine { from_id:  14, to_id: 122, constellation_name: "Gemini" },       // Pollux → Castor (heads)
    ConstellationLine { from_id: 122, to_id: 124, constellation_name: "Gemini" },       // Castor → Mebsuta
    ConstellationLine { from_id: 124, to_id: 127, constellation_name: "Gemini" },       // Mebsuta → Tejat
    ConstellationLine { from_id: 127, to_id: 126, constellation_name: "Gemini" },       // Tejat → Propus
    ConstellationLine { from_id:  14, to_id: 125, constellation_name: "Gemini" },       // Pollux → Wasat
    ConstellationLine { from_id: 125, to_id: 123, constellation_name: "Gemini" },       // Wasat → Alhena
    // ── Cancer (faint; sparse asterism with what we have) ──────────────────
    ConstellationLine { from_id: 227, to_id: 226, constellation_name: "Cancer" },       // Asellus Australis → Acubens
    ConstellationLine { from_id: 227, to_id: 228, constellation_name: "Cancer" },       // Asellus Australis → Tarf
    // ── Leo ────────────────────────────────────────────────────────────────
    ConstellationLine { from_id: 219, to_id: 221, constellation_name: "Leo" },          // Regulus → Algieba
    ConstellationLine { from_id: 221, to_id: 222, constellation_name: "Leo" },          // Algieba → Zosma
    ConstellationLine { from_id: 222, to_id: 220, constellation_name: "Leo" },          // Zosma → Denebola
    ConstellationLine { from_id: 220, to_id: 223, constellation_name: "Leo" },          // Denebola → Chertan
    ConstellationLine { from_id: 223, to_id: 219, constellation_name: "Leo" },          // Chertan → Regulus
    // ── Virgo ──────────────────────────────────────────────────────────────
    ConstellationLine { from_id:  12, to_id: 216, constellation_name: "Virgo" },        // Spica → Heze
    ConstellationLine { from_id: 216, to_id: 214, constellation_name: "Virgo" },        // Heze → Porrima
    ConstellationLine { from_id: 214, to_id: 217, constellation_name: "Virgo" },        // Porrima → Auva
    ConstellationLine { from_id: 217, to_id: 215, constellation_name: "Virgo" },        // Auva → Vindemiatrix
    ConstellationLine { from_id: 214, to_id: 218, constellation_name: "Virgo" },        // Porrima → Zavijava
    // ── Libra ──────────────────────────────────────────────────────────────
    ConstellationLine { from_id: 212, to_id: 213, constellation_name: "Libra" },        // Zubeneschamali → Zubenelgenubi
    // ── Scorpius (the famous curve) ────────────────────────────────────────
    ConstellationLine { from_id: 208, to_id: 206, constellation_name: "Scorpius" },     // Acrab → Dschubba
    ConstellationLine { from_id: 206, to_id:  13, constellation_name: "Scorpius" },     // Dschubba → Antares
    ConstellationLine { from_id:  13, to_id: 211, constellation_name: "Scorpius" },     // Antares → Paikauhale
    ConstellationLine { from_id: 211, to_id: 207, constellation_name: "Scorpius" },     // Paikauhale → Larawag
    ConstellationLine { from_id: 207, to_id: 210, constellation_name: "Scorpius" },     // Larawag → Girtab
    ConstellationLine { from_id: 210, to_id: 205, constellation_name: "Scorpius" },     // Girtab → Sargas
    ConstellationLine { from_id: 205, to_id: 204, constellation_name: "Scorpius" },     // Sargas → Shaula (tail)
    ConstellationLine { from_id: 204, to_id: 209, constellation_name: "Scorpius" },     // Shaula → Lesath (sting)
    // ── Sagittarius (the Teapot) ───────────────────────────────────────────
    ConstellationLine { from_id: 201, to_id: 200, constellation_name: "Sagittarius" },  // Kaus Borealis → Kaus Media
    ConstellationLine { from_id: 200, to_id: 197, constellation_name: "Sagittarius" },  // Kaus Media → Kaus Australis
    ConstellationLine { from_id: 197, to_id: 199, constellation_name: "Sagittarius" },  // Kaus Australis → Ascella
    ConstellationLine { from_id: 199, to_id: 198, constellation_name: "Sagittarius" },  // Ascella → Nunki
    ConstellationLine { from_id: 198, to_id: 201, constellation_name: "Sagittarius" },  // Nunki → Kaus Borealis (close lid)
    ConstellationLine { from_id: 200, to_id: 202, constellation_name: "Sagittarius" },  // Kaus Media → Alnasl (spout)
    // ── Capricornus ────────────────────────────────────────────────────────
    ConstellationLine { from_id: 161, to_id: 162, constellation_name: "Capricornus" },  // Dabih → Nashira
    ConstellationLine { from_id: 162, to_id: 160, constellation_name: "Capricornus" },  // Nashira → Deneb Algedi
    // ── Aquarius ───────────────────────────────────────────────────────────
    ConstellationLine { from_id: 158, to_id: 157, constellation_name: "Aquarius" },     // Sadalmelik → Sadalsuud
    ConstellationLine { from_id: 157, to_id: 159, constellation_name: "Aquarius" },     // Sadalsuud → Skat
    // ── Pisces — only Alpherg in our catalog, no asterism yet ──────────────
];

/// Western tropical-zodiac date range for the 12 standard signs.
///
/// Returns the **calendar** dates the Sun nominally crosses each sign in
/// western astrology — these are *tropical* (anchored to the vernal
/// equinox) and don't match the actual astronomical position of the Sun
/// in the constellation any more, because precession has shifted the
/// constellation boundaries ~30° over the past two millennia. Most
/// pop-culture references to "your sign" use these tropical dates, so
/// they're what we surface in the info card.
///
/// Ophiuchus is intentionally omitted: although the Sun does transit it,
/// it's not one of the 12 tropical signs.
pub fn zodiac_date_range(constellation_name: &str) -> Option<&'static str> {
    match constellation_name {
        "Aries"       => Some("Mar 21 – Apr 19"),
        "Taurus"      => Some("Apr 20 – May 20"),
        "Gemini"      => Some("May 21 – Jun 20"),
        "Cancer"      => Some("Jun 21 – Jul 22"),
        "Leo"         => Some("Jul 23 – Aug 22"),
        "Virgo"       => Some("Aug 23 – Sep 22"),
        "Libra"       => Some("Sep 23 – Oct 22"),
        "Scorpius"    => Some("Oct 23 – Nov 21"),
        "Sagittarius" => Some("Nov 22 – Dec 21"),
        "Capricornus" => Some("Dec 22 – Jan 19"),
        "Aquarius"    => Some("Jan 20 – Feb 18"),
        "Pisces"      => Some("Feb 19 – Mar 20"),
        _ => None,
    }
}

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

    /// All 12 tropical-zodiac signs resolve to a date range; non-zodiac
    /// constellations resolve to None. Pins the names too — the
    /// constellation table is keyed by exact name match.
    #[test]
    fn zodiac_date_ranges_cover_twelve_signs() {
        let signs = [
            "Aries",
            "Taurus",
            "Gemini",
            "Cancer",
            "Leo",
            "Virgo",
            "Libra",
            "Scorpius",
            "Sagittarius",
            "Capricornus",
            "Aquarius",
            "Pisces",
        ];
        for s in signs {
            assert!(
                zodiac_date_range(s).is_some(),
                "expected zodiac date range for {s}"
            );
        }
        // Non-zodiac sanity check.
        assert!(zodiac_date_range("Orion").is_none());
        assert!(zodiac_date_range("Cassiopeia").is_none());
        // Ophiuchus is intentionally not part of the tropical zodiac
        // even though the Sun transits it.
        assert!(zodiac_date_range("Ophiuchus").is_none());
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
    /// ID, magnitudes/coordinates must be physically sensible, and
    /// every constellation-line endpoint must resolve in the full
    /// `all_stars()` set so the asterism overlay can't quietly break
    /// when a line references a star ID that fell out of the catalog.
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
        // Every line endpoint must resolve in the combined catalog,
        // not just BRIGHTEST_STARS — sky_view looks them up via
        // `all_stars()` so any orphan ID would paint nothing.
        for line in CONSTELLATION_LINES {
            assert!(
                stars.iter().any(|s| s.id == line.from_id),
                "missing from_id {} for {}",
                line.from_id,
                line.constellation_name
            );
            assert!(
                stars.iter().any(|s| s.id == line.to_id),
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
