//! # Geolocation and City Database System
//!
//! This module implements a robust, lightweight in-memory city database.
//! To ensure zero-config compilation and complete cross-platform portability
//! (especially for `wasm32-unknown-unknown` where linking native C libraries like
//! SQLite can be complex), it embeds a catalog of major worldwide cities.
//!
//! It provides both prefix search (simulating FTS5) and spelling-insensitive phonetic
//! search using the Soundex algorithm, as specified in the implementation design.

/// Representation of a geographical city entity.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct City {
    pub name: &'static str,
    pub state: &'static str,
    pub country: &'static str,
    pub country_code: &'static str,
    pub latitude: f64,
    pub longitude: f64,
}

/// Compute the Soundex phonetic code for a string.
/// Soundex maps a name to a 4-character code (e.g., "Denver" -> "D516").
pub fn soundex(input: &str) -> String {
    if input.is_empty() {
        return "0000".to_string();
    }

    let s = input.to_uppercase();
    let mut chars = s.chars();
    let first_char = chars.next().unwrap_or(' ');
    if !first_char.is_alphabetic() {
        return "0000".to_string();
    }

    let mut code = String::new();
    code.push(first_char);

    let get_digit = |c: char| -> Option<char> {
        match c {
            'B' | 'F' | 'P' | 'V' => Some('1'),
            'C' | 'G' | 'J' | 'K' | 'Q' | 'S' | 'X' | 'Z' => Some('2'),
            'D' | 'T' => Some('3'),
            'L' => Some('4'),
            'M' | 'N' => Some('5'),
            'R' => Some('6'),
            _ => None,
        }
    };

    let mut last_digit = get_digit(first_char);

    for c in chars {
        if let Some(digit) = get_digit(c) {
            // Adjacent letters with the same code are joined
            if Some(digit) != last_digit {
                code.push(digit);
                last_digit = Some(digit);
                if code.len() == 4 {
                    break;
                }
            }
        } else if c != 'H' && c != 'W' {
            // Non-coded letters (except H and W) break adjacency grouping
            last_digit = None;
        }
    }

    // Pad with zeros if necessary
    while code.len() < 4 {
        code.push('0');
    }

    code
}

/// A curated built-in catalog of major global cities.
pub const BUILTIN_CITIES: &[City] = &[
    City { name: "Denver", state: "Colorado", country: "United States", country_code: "US", latitude: 39.7392, longitude: -104.9903 },
    City { name: "New York", state: "New York", country: "United States", country_code: "US", latitude: 40.7128, longitude: -74.0060 },
    City { name: "Los Angeles", state: "California", country: "United States", country_code: "US", latitude: 34.0522, longitude: -118.2437 },
    City { name: "London", state: "England", country: "United Kingdom", country_code: "GB", latitude: 51.5074, longitude: -0.1278 },
    City { name: "Paris", state: "Île-de-France", country: "France", country_code: "FR", latitude: 48.8566, longitude: 2.3522 },
    City { name: "Tokyo", state: "Tokyo", country: "Japan", country_code: "JP", latitude: 35.6762, longitude: 139.6503 },
    City { name: "Sydney", state: "New South Wales", country: "Australia", country_code: "AU", latitude: -33.8688, longitude: 151.2093 },
    City { name: "Berlin", state: "Berlin", country: "Germany", country_code: "DE", latitude: 52.5200, longitude: 13.4050 },
    City { name: "Cairo", state: "Cairo", country: "Egypt", country_code: "EG", latitude: 30.0444, longitude: 31.2357 },
    City { name: "Mumbai", state: "Maharashtra", country: "India", country_code: "IN", latitude: 19.0760, longitude: 72.8777 },
    City { name: "Rio de Janeiro", state: "Rio de Janeiro", country: "Brazil", country_code: "BR", latitude: -22.9068, longitude: -43.1729 },
    City { name: "Cape Town", state: "Western Cape", country: "South Africa", country_code: "ZA", latitude: -33.9249, longitude: 18.4241 },
    City { name: "Toronto", state: "Ontario", country: "Canada", country_code: "CA", latitude: 43.6532, longitude: -79.3832 },
    City { name: "Vancouver", state: "British Columbia", country: "Canada", country_code: "CA", latitude: 49.2827, longitude: -123.1207 },
    City { name: "Rome", state: "Lazio", country: "Italy", country_code: "IT", latitude: 41.9028, longitude: 12.4964 },
    City { name: "Beijing", state: "Beijing", country: "China", country_code: "CN", latitude: 39.9042, longitude: 116.4074 },
    City { name: "Singapore", state: "Central Region", country: "Singapore", country_code: "SG", latitude: 1.3521, longitude: 103.8198 },
    City { name: "Dubai", state: "Dubai", country: "United Arab Emirates", country_code: "AE", latitude: 25.2048, longitude: 55.2708 },
];

/// Perform a search on the city database.
/// First attempts a prefix search (MATCH 'prefix*').
/// If prefix search yields zero results, falls back to Soundex phonetic lookup.
pub fn search_cities(query: &str) -> Vec<City> {
    let clean_query = query.trim().to_lowercase();
    if clean_query.is_empty() {
        return BUILTIN_CITIES.to_vec();
    }

    // 1. Prefix/Contains Search (FTS5 approximation)
    let prefix_results: Vec<City> = BUILTIN_CITIES
        .iter()
        .filter(|city| {
            city.name.to_lowercase().starts_with(&clean_query)
                || city.name.to_lowercase().contains(&clean_query)
                || city.state.to_lowercase().starts_with(&clean_query)
                || city.country.to_lowercase().starts_with(&clean_query)
        })
        .cloned()
        .collect();

    if !prefix_results.is_empty() {
        return prefix_results;
    }

    // 2. Soundex Phonetic Fallback (Typo/Phonetic evaluation)
    let query_soundex = soundex(&clean_query);
    BUILTIN_CITIES
        .iter()
        .filter(|city| soundex(city.name) == query_soundex)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soundex_algorithm() {
        assert_eq!(soundex("Denver"), "D516");
        assert_eq!(soundex("London"), "L535");
        assert_eq!(soundex("Paris"), "P620");
        // Similar sounding words or typos
        assert_eq!(soundex("Denve"), "D510");
    }

    #[test]
    fn test_city_search() {
        // Exact prefix
        let res = search_cities("Denv");
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].name, "Denver");

        // Case insensitive
        let res2 = search_cities("tokyo");
        assert_eq!(res2.len(), 1);
        assert_eq!(res2[0].name, "Tokyo");

        // Phonetic search fallback
        // "Denwer" -> should soundex match "Denver"
        let res_phonetic = search_cities("Denwer");
        assert_eq!(res_phonetic.len(), 1);
        assert_eq!(res_phonetic[0].name, "Denver");
    }
}
