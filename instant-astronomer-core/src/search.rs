//! Object search: match a free-text query against the catalog of named
//! stars, Solar System bodies, and constellations, and resolve a chosen
//! target back to live equatorial coordinates for the on-sky finder.
//!
//! Mirrors the city-search backend in [`crate::cities`] but for celestial
//! objects. The search UI ([`crate::search_panel`]) calls
//! [`search_objects`] on every keystroke and stores the chosen
//! [`SearchTarget`]; the sky view ([`crate::widgets::sky_view`]) then calls
//! [`resolve_target_coords`] every frame to draw the "where do I point?"
//! finder. Keeping the catalog math here means both consumers agree on how
//! a name maps to a sky position.

use std::f64::consts::PI;

use nalgebra::Vector3;

use crate::math::EquatorialCoords;
use crate::stars::{all_stars, calculate_solar_system_bodies, CONSTELLATION_LINES, SPACECRAFT};

/// How a [`SearchTarget`] resolves back to equatorial coordinates.
#[derive(Debug, Clone)]
pub enum SearchKind {
    /// Fixed J2000 coordinates. Stars and constellation centroids don't
    /// move on human timescales, so we snapshot them at selection time.
    Fixed(EquatorialCoords),
    /// A Solar System body (Sun / Moon / planet). Re-resolved by name from
    /// the ephemeris every frame because it drifts across the sky.
    SolarSystem,
}

/// A selectable search result: a display `name`, the `kind` used to
/// resolve a live direction, and a short `category` label for the list.
#[derive(Debug, Clone)]
pub struct SearchTarget {
    pub name: String,
    pub kind: SearchKind,
    /// Short category label shown in the results list ("Star", "Planet",
    /// "Constellation", "Sun", "Moon").
    pub category: &'static str,
}

/// Maximum number of results surfaced in the live list. Keeps the overlay
/// compact and the per-keystroke scan bounded.
pub const MAX_RESULTS: usize = 8;

/// Rank a candidate name against a lowercased query. `Some(0)` for a
/// prefix match (ranked first), `Some(1)` for a substring match, `None`
/// when it doesn't match at all.
fn match_rank(name: &str, query_lc: &str) -> Option<u8> {
    let lname = name.to_lowercase();
    if lname.starts_with(query_lc) {
        Some(0)
    } else if lname.contains(query_lc) {
        Some(1)
    } else {
        None
    }
}

/// Short category label for a Solar System body name.
fn solar_category(name: &str) -> &'static str {
    match name {
        "Sun" => "Sun",
        "Moon" => "Moon",
        _ => "Planet",
    }
}

/// Search the celestial catalog for objects matching `query`.
///
/// Matches (case-insensitive) across named Solar System bodies, tracked
/// spacecraft, named stars, and constellation names. Prefix matches rank
/// ahead of substring matches; within a rank, ordering follows the scan
/// order (bodies, then spacecraft, then stars, then constellations) which
/// roughly tracks how interesting each hit is. Returns at most
/// [`MAX_RESULTS`] entries. `now_ms` is needed to place the Solar System
/// bodies so they can be matched by name.
pub fn search_objects(query: &str, now_ms: i64) -> Vec<SearchTarget> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }

    // (rank, target). A stable sort by rank keeps prefix hits ahead of
    // substring hits while preserving the bodies → stars → constellations
    // insertion order within each rank.
    let mut scored: Vec<(u8, SearchTarget)> = Vec::new();

    for body in calculate_solar_system_bodies(now_ms) {
        if let Some(rank) = match_rank(body.name, &q) {
            scored.push((
                rank,
                SearchTarget {
                    name: body.name.to_string(),
                    kind: SearchKind::SolarSystem,
                    category: solar_category(body.name),
                },
            ));
        }
    }

    for craft in SPACECRAFT {
        if let Some(rank) = match_rank(craft.name, &q) {
            scored.push((
                rank,
                SearchTarget {
                    name: craft.name.to_string(),
                    kind: SearchKind::Fixed(craft.coords),
                    category: "Spacecraft",
                },
            ));
        }
    }

    for star in all_stars() {
        if let Some(rank) = match_rank(star.name, &q) {
            scored.push((
                rank,
                SearchTarget {
                    name: star.name.to_string(),
                    kind: SearchKind::Fixed(star.coords),
                    category: "Star",
                },
            ));
        }
    }

    let mut seen: Vec<&str> = Vec::new();
    for line in CONSTELLATION_LINES {
        if seen.contains(&line.constellation_name) {
            continue;
        }
        seen.push(line.constellation_name);
        if let Some(rank) = match_rank(line.constellation_name, &q) {
            if let Some(coords) = constellation_centroid(line.constellation_name) {
                scored.push((
                    rank,
                    SearchTarget {
                        name: line.constellation_name.to_string(),
                        kind: SearchKind::Fixed(coords),
                        category: "Constellation",
                    },
                ));
            }
        }
    }

    scored.sort_by_key(|(rank, _)| *rank);
    scored.into_iter().take(MAX_RESULTS).map(|(_, t)| t).collect()
}

/// Resolve a target back to current equatorial coordinates. `Fixed`
/// targets return their snapshot; `SolarSystem` targets re-run the
/// ephemeris at `now_ms` and look themselves up by name.
pub fn resolve_target_coords(target: &SearchTarget, now_ms: i64) -> Option<EquatorialCoords> {
    match &target.kind {
        SearchKind::Fixed(coords) => Some(*coords),
        SearchKind::SolarSystem => calculate_solar_system_bodies(now_ms)
            .into_iter()
            .find(|b| b.name == target.name)
            .map(|b| b.coords),
    }
}

/// Average position of a constellation's member stars, returned as a
/// single equatorial coordinate to "point at the constellation". Computed
/// as the normalized mean of the member stars' unit vectors so it behaves
/// correctly across the RA = 0/2π seam. `None` if no member star resolves.
fn constellation_centroid(name: &str) -> Option<EquatorialCoords> {
    let stars = all_stars();
    let mut ids: Vec<u32> = Vec::new();
    for line in CONSTELLATION_LINES {
        if line.constellation_name != name {
            continue;
        }
        if !ids.contains(&line.from_id) {
            ids.push(line.from_id);
        }
        if !ids.contains(&line.to_id) {
            ids.push(line.to_id);
        }
    }

    let mut sum = Vector3::new(0.0, 0.0, 0.0);
    let mut count = 0u32;
    for id in ids {
        if let Some(star) = stars.iter().find(|s| s.id == id) {
            sum += equatorial_unit_vector(star.coords);
            count += 1;
        }
    }
    if count == 0 || sum.norm() < 1e-9 {
        return None;
    }
    Some(unit_vector_to_equatorial(sum))
}

/// Equatorial (RA/Dec) to a 3D unit vector. X toward (RA=0, Dec=0), Z
/// toward the north celestial pole. Frame-internal; only used for the
/// centroid average, so it just needs to be self-consistent.
fn equatorial_unit_vector(c: EquatorialCoords) -> Vector3<f64> {
    let cd = c.dec.cos();
    Vector3::new(cd * c.ra.cos(), cd * c.ra.sin(), c.dec.sin())
}

/// Inverse of [`equatorial_unit_vector`] — RA wrapped to `[0, 2π)`.
fn unit_vector_to_equatorial(v: Vector3<f64>) -> EquatorialCoords {
    let r = v.norm().max(1e-12);
    let dec = (v.z / r).asin();
    let mut ra = v.y.atan2(v.x);
    if ra < 0.0 {
        ra += 2.0 * PI;
    }
    EquatorialCoords { ra, dec }
}

#[cfg(test)]
mod tests {
    use super::*;

    // J2000 epoch (2000-01-01T12:00:00Z) — fixed so the planet matches are
    // deterministic.
    const NOW: i64 = 946_728_000_000;

    #[test]
    fn finds_a_star_by_prefix() {
        let results = search_objects("sir", NOW);
        assert!(
            results.iter().any(|t| t.name == "Sirius" && t.category == "Star"),
            "expected Sirius in {:?}",
            results.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn finds_a_planet_by_prefix() {
        let results = search_objects("ven", NOW);
        let venus = results.iter().find(|t| t.name == "Venus");
        let venus = venus.expect("Venus should be found");
        assert_eq!(venus.category, "Planet");
        assert!(matches!(venus.kind, SearchKind::SolarSystem));
    }

    #[test]
    fn finds_voyager_spacecraft_by_prefix() {
        let results = search_objects("voyager", NOW);
        for expected in ["Voyager 1", "Voyager 2"] {
            let craft = results
                .iter()
                .find(|t| t.name == expected)
                .unwrap_or_else(|| {
                    panic!(
                        "expected {expected} in {:?}",
                        results.iter().map(|t| &t.name).collect::<Vec<_>>()
                    )
                });
            assert_eq!(craft.category, "Spacecraft");
            // Fixed snapshot — the finder resolves it without an ephemeris.
            assert!(matches!(craft.kind, SearchKind::Fixed(_)));
            assert!(resolve_target_coords(craft, NOW).is_some());
        }
    }

    #[test]
    fn finds_a_constellation_by_prefix() {
        let results = search_objects("ori", NOW);
        let orion = results
            .iter()
            .find(|t| t.name == "Orion")
            .expect("Orion should be found");
        assert_eq!(orion.category, "Constellation");
        assert!(matches!(orion.kind, SearchKind::Fixed(_)));
    }

    #[test]
    fn empty_query_returns_nothing() {
        assert!(search_objects("", NOW).is_empty());
        assert!(search_objects("   ", NOW).is_empty());
    }

    #[test]
    fn prefix_matches_rank_before_substring_matches() {
        // "an" matches Antares/Andromeda (prefix) and several stars that
        // merely contain "an". Every prefix hit must precede every
        // substring-only hit.
        let results = search_objects("an", NOW);
        let mut seen_substring_only = false;
        for t in &results {
            if t.name.to_lowercase().starts_with("an") {
                assert!(
                    !seen_substring_only,
                    "prefix match {} appeared after a substring-only match",
                    t.name
                );
            } else {
                seen_substring_only = true;
            }
        }
    }

    #[test]
    fn results_are_capped() {
        // A very common substring should overflow the cap and be trimmed.
        let results = search_objects("a", NOW);
        assert!(results.len() <= MAX_RESULTS);
    }

    #[test]
    fn resolve_solar_target_by_name() {
        let target = SearchTarget {
            name: "Mars".to_string(),
            kind: SearchKind::SolarSystem,
            category: "Planet",
        };
        assert!(resolve_target_coords(&target, NOW).is_some());
    }

    #[test]
    fn resolve_fixed_target_returns_snapshot() {
        let coords = EquatorialCoords { ra: 1.0, dec: 0.5 };
        let target = SearchTarget {
            name: "Whatever".to_string(),
            kind: SearchKind::Fixed(coords),
            category: "Star",
        };
        let resolved = resolve_target_coords(&target, NOW).expect("fixed always resolves");
        assert!((resolved.ra - coords.ra).abs() < 1e-12);
        assert!((resolved.dec - coords.dec).abs() < 1e-12);
    }

    #[test]
    fn orion_centroid_sits_among_its_stars() {
        // Orion's member stars cluster near RA ~1.4 rad, Dec ~0 rad. The
        // centroid must land in that neighbourhood, not at some seam.
        let c = constellation_centroid("Orion").expect("Orion has member stars");
        assert!(
            (1.2..1.7).contains(&c.ra),
            "Orion centroid RA out of expected range: {}",
            c.ra
        );
        assert!(
            (-0.3..0.3).contains(&c.dec),
            "Orion centroid Dec out of expected range: {}",
            c.dec
        );
    }

    #[test]
    fn unknown_constellation_has_no_centroid() {
        assert!(constellation_centroid("Nonexistent").is_none());
    }
}
