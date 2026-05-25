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
use agg_gui::widgets::{Button, Checkbox, Container, FlexColumn, FlexRow, TextField};
use agg_gui::App;

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
}

/// Handles to the live state cells the core app exposes to platform shells.
///
/// Shells write into `yaw`/`pitch`/`roll` from device-orientation events,
/// keep `timestamp_ms` advancing every frame, and may write `latitude` /
/// `longitude` from the platform geolocation pipeline. `calibration_yaw`
/// is a per-session offset applied before the projection so the user can
/// re-align "what my phone is pointing at" to "what the app shows" — see
/// [`build_astronomer_app`]'s calibrate button.
pub struct AstronomerHandles {
    pub latitude: Rc<Cell<f64>>,
    pub longitude: Rc<Cell<f64>>,
    pub timestamp_ms: Rc<Cell<i64>>,
    pub yaw: Rc<Cell<f64>>,
    pub pitch: Rc<Cell<f64>>,
    pub roll: Rc<Cell<f64>>,
    /// Subtracted from `yaw` before the projection runs. Lets the user
    /// tap a "Calibrate to North" button while pointing roughly at
    /// north and have the rendered sky snap into alignment with where
    /// they're actually looking. Stored in **radians**.
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
    let yaw = Rc::new(Cell::new(0.0));
    let pitch = Rc::new(Cell::new(0.0));
    let roll = Rc::new(Cell::new(0.0));
    let calibration_yaw = Rc::new(Cell::new(0.0));
    let show_constellations = Rc::new(Cell::new(true));
    let search_text = Rc::new(std::cell::RefCell::new(String::new()));
    let search_status = Rc::new(std::cell::RefCell::new(String::from("Type a city to search")));

    let handles = AstronomerHandles {
        latitude: Rc::clone(&latitude),
        longitude: Rc::clone(&longitude),
        timestamp_ms: Rc::clone(&timestamp_ms),
        yaw: Rc::clone(&yaw),
        pitch: Rc::clone(&pitch),
        roll: Rc::clone(&roll),
        calibration_yaw: Rc::clone(&calibration_yaw),
    };

    let sky_widget = SkyViewWidget::new(
        Arc::clone(&font),
        Rc::clone(&latitude),
        Rc::clone(&longitude),
        Rc::clone(&timestamp_ms),
        Rc::clone(&yaw),
        Rc::clone(&pitch),
        Rc::clone(&roll),
        Rc::clone(&show_constellations),
    );
    let tape_widget = HorizonTapeWidget::new(Arc::clone(&font), Rc::clone(&yaw));

    let panel = build_control_panel(
        Arc::clone(&font),
        platform,
        Rc::clone(&latitude),
        Rc::clone(&longitude),
        Rc::clone(&show_constellations),
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
fn build_control_panel<P: AstronomerPlatform>(
    font: Arc<Font>,
    platform: P,
    latitude: Rc<Cell<f64>>,
    longitude: Rc<Cell<f64>>,
    show_constellations: Rc<Cell<bool>>,
    search_text: Rc<std::cell::RefCell<String>>,
    search_status: Rc<std::cell::RefCell<String>>,
) -> Container {
    let platform = Rc::new(platform);

    let geo_button = {
        let platform = Rc::clone(&platform);
        Button::new("Geolocation", Arc::clone(&font)).on_click(move || {
            platform.request_geolocation();
        })
    };

    let constellation_checkbox = Checkbox::new(
        "Constellations",
        Arc::clone(&font),
        show_constellations.get(),
    )
    .with_state_cell(Rc::clone(&show_constellations));

    let coord_label = {
        let lat = Rc::clone(&latitude);
        let lng = Rc::clone(&longitude);
        StatusText::new(Arc::clone(&font), move || {
            format!("Lat: {:.4}°  Lng: {:.4}°", lat.get(), lng.get())
        })
        .with_font_size(12.0)
    };

    let row_1 = FlexRow::new()
        .with_gap(12.0)
        .add(Box::new(geo_button))
        .add(Box::new(constellation_checkbox))
        .add_flex(Box::new(coord_label), 1.0);

    let search_field = {
        let text = Rc::clone(&search_text);
        TextField::new(Arc::clone(&font))
            .with_placeholder("Search city (e.g. London, Tokyo, Paris)...")
            .on_change(move |s| {
                *text.borrow_mut() = s.to_string();
            })
    };

    let search_button = {
        let lat = Rc::clone(&latitude);
        let lng = Rc::clone(&longitude);
        let text = Rc::clone(&search_text);
        let status = Rc::clone(&search_status);
        Button::new("Search", Arc::clone(&font)).on_click(move || {
            let query = text.borrow().clone();
            let matches = cities::search_cities(&query);
            if let Some(city) = matches.first() {
                lat.set(city.latitude);
                lng.set(city.longitude);
                *status.borrow_mut() =
                    format!("Located: {}, {}", city.name, city.country_code);
            } else {
                *status.borrow_mut() = String::from("City not found");
            }
            agg_gui::animation::request_draw();
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

    let inner = FlexColumn::new()
        .with_gap(8.0)
        .add(Box::new(row_1))
        .add(Box::new(row_2));

    Container::new()
        .add(Box::new(inner))
        .with_fit_height(true)
        .with_background(Color::from_rgb8(28, 28, 40))
        .with_border(Color::from_rgb8(50, 50, 70), 1.0)
        .with_inner_padding(Insets::all(12.0))
}

/// Current UTC unix time in milliseconds. Wrapped here so the entry points
/// don't repeat the `web_time` plumbing.
pub fn current_unix_ms() -> i64 {
    web_time::SystemTime::now()
        .duration_since(web_time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
