//! # Instant-Astronomer Core
//!
//! Target-agnostic core for Instant-Astronomer. Implements the astronomy /
//! projection math, the city lookup database, the custom sky + horizon
//! widgets, and the shared widget-tree builder.
//!
//! Per `implementation.md`, every visible pixel renders through agg-gui's
//! [`DrawCtx`] — there is no separate canvas/WebGL/wgpu rendering path. The
//! native + WASM shells in sibling crates only own the OS window/canvas, the
//! event-loop, and the platform geolocation hook.
//!
//! The crate is `wasm32`-clean: no `tokio`, no `winit`, no direct `wgpu`
//! calls. Platform shells inject capabilities through the
//! [`AstronomerPlatform`] trait.

pub mod cities;
pub mod math;
pub mod stars;

pub mod widgets {
    //! Custom widgets used by the Instant-Astronomer UI shell.
    pub mod horizon_tape;
    pub mod sky_view;
    pub mod status_text;
}

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::layout_props::Insets;
use agg_gui::text::Font;
use agg_gui::widgets::{Button, Checkbox, Conditional, Container, FlexColumn, FlexRow, TextField};
use agg_gui::App;
use nalgebra::UnitQuaternion;

use crate::widgets::horizon_tape::HorizonTapeWidget;
use crate::widgets::sky_view::SkyViewWidget;
use crate::widgets::status_text::StatusText;

/// CascadiaCode bundled into the binary.
///
/// Native + WASM shells pull this via [`load_default_font`] so both targets
/// render the same glyphs without filesystem access (agg-gui's text stack
/// needs a parsed `Font` before the first paint).
pub const DEFAULT_FONT_BYTES: &[u8] = include_bytes!("../assets/CascadiaCode.ttf");

/// Load the default font (CascadiaCode) as an `Arc<Font>`.
pub fn load_default_font() -> Arc<Font> {
    Arc::new(Font::from_slice(DEFAULT_FONT_BYTES).expect("instant-astronomer default font"))
}

/// Platform capability surface. Native + WASM shells implement this so the
/// core widget tree can request services (geolocation lookup, eventually
/// device-orientation listener installation, etc.) without `cfg`-gating.
pub trait AstronomerPlatform: 'static {
    /// Trigger a geolocation lookup. Implementations should asynchronously
    /// update [`AstronomerHandles::latitude`] / `longitude` and call
    /// `agg_gui::animation::request_draw` when results arrive.
    fn request_geolocation(&self);

    /// Minutes east of UTC for the device's wall clock, with DST applied
    /// (e.g. PDT = -420, IST = +330). Used purely for the clock readout
    /// in the configuration tray — the sky math runs in UTC and ignores
    /// this. Default returns 0 (UTC) for callers that don't care.
    fn local_offset_minutes(&self) -> i32 {
        0
    }
}

/// Handles to the live state cells the core app exposes to platform shells.
///
/// Shells write into `view_quat` from device-orientation events (after
/// converting the Euler triple to a unit quaternion), keep
/// `timestamp_ms` advancing every frame, and may write `latitude` /
/// `longitude` from the platform geolocation pipeline. `calibration_yaw`
/// is a per-session compass offset the user sets with the Calibrate
/// button so the rendered sky stays aligned with where they're actually
/// pointing the phone.
///
/// `view_quat` is the world→view rotation. Replaces the previous
/// `yaw`/`pitch`/`roll` Euler triple to fix gimbal lock when the user
/// tilts the phone through the zenith/nadir poles.
pub struct AstronomerHandles {
    pub latitude: Rc<Cell<f64>>,
    pub longitude: Rc<Cell<f64>>,
    pub timestamp_ms: Rc<Cell<i64>>,
    /// World→view rotation as a unit quaternion. Mouse drag composes
    /// camera-local rotations into this cell; device-orientation events
    /// `set()` it directly each time the browser fires.
    pub view_quat: Rc<Cell<UnitQuaternion<f64>>>,
    /// Compass-offset calibration in **radians**. Applied as an
    /// additional rotation around the world up axis so the user can
    /// re-align "what my phone is pointing at" with the rendered north.
    pub calibration_yaw: Rc<Cell<f64>>,
}

/// Build the shared Instant-Astronomer widget tree. Both the native and
/// WASM shells call this and forward platform input into the returned
/// [`App`].
pub fn build_astronomer_app<P: AstronomerPlatform>(
    font: Arc<Font>,
    platform: P,
) -> (App, AstronomerHandles) {
    // Default coordinates: Royal Observatory Greenwich — neutral starting
    // point until the platform geolocation hook resolves.
    let latitude = Rc::new(Cell::new(51.4769));
    let longitude = Rc::new(Cell::new(0.0));
    let timestamp_ms = Rc::new(Cell::new(current_unix_ms()));
    // World→view rotation. Identity = camera looks north along +Z.
    let view_quat = Rc::new(Cell::new(UnitQuaternion::<f64>::identity()));
    let calibration_yaw = Rc::new(Cell::new(0.0));
    let show_constellations = Rc::new(Cell::new(true));
    // Default to geolocation (the common case on phones). Unchecking
    // reveals the city search field. They never need to be on at the
    // same time — geolocation already gives the exact lat/lng.
    let use_geolocation = Rc::new(Cell::new(true));
    let search_text = Rc::new(std::cell::RefCell::new(String::new()));
    let search_status = Rc::new(std::cell::RefCell::new(String::from("Type a city to search")));

    let handles = AstronomerHandles {
        latitude: Rc::clone(&latitude),
        longitude: Rc::clone(&longitude),
        timestamp_ms: Rc::clone(&timestamp_ms),
        view_quat: Rc::clone(&view_quat),
        calibration_yaw: Rc::clone(&calibration_yaw),
    };

    let sky_widget = SkyViewWidget::new(
        Arc::clone(&font),
        Rc::clone(&latitude),
        Rc::clone(&longitude),
        Rc::clone(&timestamp_ms),
        Rc::clone(&view_quat),
        Rc::clone(&calibration_yaw),
        Rc::clone(&show_constellations),
    );
    let tape_widget = HorizonTapeWidget::new(Arc::clone(&font), Rc::clone(&view_quat));

    let panel = build_control_panel(
        Arc::clone(&font),
        platform,
        Rc::clone(&latitude),
        Rc::clone(&longitude),
        Rc::clone(&timestamp_ms),
        Rc::clone(&view_quat),
        Rc::clone(&calibration_yaw),
        Rc::clone(&show_constellations),
        Rc::clone(&use_geolocation),
        Rc::clone(&search_text),
        Rc::clone(&search_status),
    );

    let root = FlexColumn::new()
        .with_gap(0.0)
        .add_flex(Box::new(sky_widget), 1.0)
        .add(Box::new(tape_widget))
        .add(Box::new(panel));

    (App::new(Box::new(root)), handles)
}

/// Build the bottom configuration tray (geolocation button, constellation
/// toggle, coordinate readout, city search).
#[allow(clippy::too_many_arguments)]
fn build_control_panel<P: AstronomerPlatform>(
    font: Arc<Font>,
    platform: P,
    latitude: Rc<Cell<f64>>,
    longitude: Rc<Cell<f64>>,
    timestamp_ms: Rc<Cell<i64>>,
    view_quat: Rc<Cell<UnitQuaternion<f64>>>,
    calibration_yaw: Rc<Cell<f64>>,
    show_constellations: Rc<Cell<bool>>,
    use_geolocation: Rc<Cell<bool>>,
    search_text: Rc<std::cell::RefCell<String>>,
    search_status: Rc<std::cell::RefCell<String>>,
) -> Container {
    let platform = Rc::new(platform);

    // Geolocation re-fetch button (works in both modes — even when the
    // user has unchecked "Use geolocation", a quick re-tap fills the
    // city search field with the current location to seed a city
    // lookup).
    let geo_button = {
        let platform = Rc::clone(&platform);
        Button::new("Locate me", Arc::clone(&font)).on_click(move || {
            platform.request_geolocation();
        })
    };

    // Toggle: when checked the app uses the device's reported
    // geolocation; when unchecked the city-search field appears so the
    // user can pick a location by name. Both at once is redundant.
    // Maintain an inverted `show_search` cell so the `Conditional`
    // wrapping the search row can stay declarative — Checkbox's
    // `with_state_cell` writes `use_geolocation` automatically; the
    // `on_change` closure just mirrors that to `show_search`.
    let show_search = Rc::new(Cell::new(!use_geolocation.get()));
    let geo_checkbox = {
        let show_search = Rc::clone(&show_search);
        Checkbox::new("Use geolocation", Arc::clone(&font), use_geolocation.get())
            .with_state_cell(Rc::clone(&use_geolocation))
            .on_change(move |checked| {
                show_search.set(!checked);
                agg_gui::animation::request_draw();
            })
    };

    let constellation_checkbox = Checkbox::new(
        "Constellations",
        Arc::clone(&font),
        show_constellations.get(),
    )
    .with_state_cell(Rc::clone(&show_constellations));

    // Calibrate-to-north button: snapshots the current compass heading
    // derived from `view_quat` into `calibration_yaw`. The projection
    // subtracts this offset on every frame, so the direction the
    // user's phone is currently pointing becomes the rendered
    // "north". A second tap somewhere else re-snaps.
    let calibrate_button = {
        let vq = Rc::clone(&view_quat);
        let cal = Rc::clone(&calibration_yaw);
        Button::new("Calibrate", Arc::clone(&font)).on_click(move || {
            cal.set(view_quat_heading_rad(vq.get()));
            agg_gui::animation::request_draw();
        })
    };

    let coord_label = {
        let lat = Rc::clone(&latitude);
        let lng = Rc::clone(&longitude);
        StatusText::new(Arc::clone(&font), move || {
            format!("Lat: {:.4}°  Lng: {:.4}°", lat.get(), lng.get())
        })
        .with_font_size(12.0)
    };

    // Live clock — UTC plus the device's local time with DST applied
    // (offset comes from the platform shell: time crate on native,
    // `Date.getTimezoneOffset` on WASM). The offset is queried every
    // paint so a user crossing a DST boundary while the app is open
    // sees the clock update without a restart.
    let time_label = {
        let ts = Rc::clone(&timestamp_ms);
        let platform_for_clock = Rc::clone(&platform);
        StatusText::new(Arc::clone(&font), move || {
            format_clock_label(ts.get(), platform_for_clock.local_offset_minutes())
        })
        .with_font_size(11.0)
    };

    let row_1 = FlexRow::new()
        .with_gap(12.0)
        .add(Box::new(geo_checkbox))
        .add(Box::new(geo_button))
        .add(Box::new(calibrate_button))
        .add(Box::new(constellation_checkbox))
        .add_flex(Box::new(coord_label), 1.0)
        .add(Box::new(time_label));

    // Shared "do the search now" closure so the Search button, Enter
    // key, and live on_change all use exactly the same path. Without
    // this the user reported "typing then hitting enter is not
    // searching" -- the field only fired on the button.
    let run_search: Rc<dyn Fn(&str)> = {
        let lat = Rc::clone(&latitude);
        let lng = Rc::clone(&longitude);
        let status = Rc::clone(&search_status);
        Rc::new(move |query: &str| {
            let q = query.trim();
            if q.is_empty() {
                *status.borrow_mut() = String::from("Type a city to search");
                return;
            }
            let matches = cities::search_cities(q);
            if let Some(city) = matches.first() {
                lat.set(city.latitude);
                lng.set(city.longitude);
                *status.borrow_mut() = if matches.len() > 1 {
                    format!("{}, {}  (+{} more)", city.name, city.country_code, matches.len() - 1)
                } else {
                    format!("{}, {}", city.name, city.country_code)
                };
            } else {
                *status.borrow_mut() = format!("\"{q}\" not found in built-in catalog");
            }
            agg_gui::animation::request_draw();
        })
    };

    let search_field = {
        let text = Rc::clone(&search_text);
        let search_on_change = Rc::clone(&run_search);
        let search_on_enter = Rc::clone(&run_search);
        TextField::new(Arc::clone(&font))
            .with_placeholder("Search city (e.g. Irvine, London, Tokyo)...")
            .on_change(move |s| {
                *text.borrow_mut() = s.to_string();
                // Live search-as-you-type: cheap (~150-entry linear
                // scan) and gives the user immediate feedback rather
                // than the previous "type, then click Search, then
                // wait" round-trip.
                (search_on_change)(s);
            })
            .on_enter(move |s| {
                (search_on_enter)(s);
            })
    };

    let search_button = {
        let text = Rc::clone(&search_text);
        let click_search = Rc::clone(&run_search);
        Button::new("Search", Arc::clone(&font)).on_click(move || {
            let query = text.borrow().clone();
            (click_search)(&query);
        })
    };

    let status_label = {
        let status = Rc::clone(&search_status);
        StatusText::new(Arc::clone(&font), move || status.borrow().clone()).with_font_size(11.0)
    };

    let row_2 = FlexRow::new()
        .with_gap(12.0)
        .add_flex(Box::new(search_field), 1.0)
        .add(Box::new(search_button))
        .add_flex(Box::new(status_label), 1.0);

    // Hide the search row entirely while "Use geolocation" is checked
    // — the FlexColumn's gap is also suppressed for hidden children.
    let row_2_conditional = Conditional::new(Rc::clone(&show_search), Box::new(row_2));

    let inner = FlexColumn::new()
        .with_gap(8.0)
        .add(Box::new(row_1))
        .add(Box::new(row_2_conditional));

    Container::new()
        .add(Box::new(inner))
        .with_fit_height(true)
        .with_background(Color::from_rgb8(28, 28, 40))
        .with_border(Color::from_rgb8(50, 50, 70), 1.0)
        .with_inner_padding(Insets::all(12.0))
}

/// Extract the W3C-convention compass heading (CCW from north, in
/// radians) from a world→view quaternion. Used by the Calibrate
/// button and the HorizonTapeWidget so they agree on "which direction
/// is the camera pointing right now?"
///
/// Implementation: the camera-forward direction in **world** coords is
/// `view_quat.inverse() * (0, 0, 1)`. Heading = `atan2(-x, z)` puts
/// north (0,0,1)→0, east (1,0,0)→-π/2 (i.e. CCW = +90°/east in W3C
/// world). Negating recovers W3C alpha.
pub fn view_quat_heading_rad(view_quat: UnitQuaternion<f64>) -> f64 {
    let forward_world = view_quat.inverse_transform_vector(&nalgebra::Vector3::new(0.0, 0.0, 1.0));
    -forward_world.x.atan2(forward_world.z)
}

/// Current UTC unix time in milliseconds. Wrapped here so the entry points
/// don't repeat the `web_time` plumbing.
pub fn current_unix_ms() -> i64 {
    web_time::SystemTime::now()
        .duration_since(web_time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Build the "UTC HH:MM · local HH:MM" status string shown in the
/// control panel so the user can verify the app has them located in
/// the right place at the right time.
///
/// The "local" half is **solar time** — UTC offset by `longitude /
/// 15 hours`. That's not the user's legal-civil time (which would
/// require a tz database to look up the offset from coords + the DST
/// rules) but it's close enough that someone in California will see
/// roughly Pacific time, someone in London will see roughly UK time,
/// etc. Worth ~30 minutes of error vs. the alternative of bundling
/// `tzf-rs` (~5 MB of polygon data) into the WASM blob.
fn format_clock_label(timestamp_ms: i64, offset_minutes: i32) -> String {
    let utc_h = ((timestamp_ms / 3_600_000) % 24 + 24) % 24;
    let utc_m = ((timestamp_ms / 60_000) % 60 + 60) % 60;
    // Local wall clock = UTC + platform-reported offset. The platform
    // applies DST, so we just add minutes blindly here.
    let local_ms = timestamp_ms + (offset_minutes as i64) * 60_000;
    let l_h = ((local_ms / 3_600_000) % 24 + 24) % 24;
    let l_m = ((local_ms / 60_000) % 60 + 60) % 60;
    format!(
        "UTC {:02}:{:02} · local {:02}:{:02}",
        utc_h, utc_m, l_h, l_m
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Local clock follows the platform-reported offset, DST included.
    /// 1700000000000 ms is 2023-11-14T22:13:20Z; with offset -480 (PST)
    /// that's 14:13 local. With +330 (IST) that's 03:43 next-day local
    /// — wrap correctly.
    #[test]
    fn format_clock_label_applies_offset_with_wrap() {
        let s = format_clock_label(1_700_000_000_000, -480);
        assert!(s.contains("UTC 22:13"), "got: {s}");
        assert!(s.contains("local 14:13"), "got: {s}");

        let s = format_clock_label(1_700_000_000_000, 330);
        assert!(s.contains("local 03:43"), "got: {s}");
    }
}
