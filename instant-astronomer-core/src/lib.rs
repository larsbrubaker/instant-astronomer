//! # Instant-Astronomer Core
//!
//! This crate implements the core business logic, astronomical calculations,
//! and UI widgets for Instant-Astronomer. It is completely target-agnostic and WASM-clean,
//! complying with the design guidelines of the rust-apps workspace.
//!
//! Main modules:
//! - `math`: Coordinates transformations, Julian dates, and orientation matrices.
//! - `cities`: Phonetic and prefix city lookups.
//! - `stars`: Core database of brightest stars and Keplerian solar system positions.
//! - `widgets`: Custom 3D projection sky canvas and rolling HUD horizon tape.

pub mod math;
pub mod cities;
pub mod stars;

pub mod widgets {
    pub mod sky_view;
    pub mod horizon_tape;
    pub mod dynamic_label;
}

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::text::Font;
use agg_gui::widgets::{Button, Checkbox, Container, FlexColumn, FlexRow, TextField};
use agg_gui::{App, Insets};

use crate::widgets::horizon_tape::HorizonTapeWidget;
use crate::widgets::sky_view::SkyViewWidget;
use crate::widgets::dynamic_label::DynamicLabel;

/// CascadiaCode bundled into the binary.
pub const DEFAULT_FONT_BYTES: &[u8] = include_bytes!("../assets/CascadiaCode.ttf");

/// Load the default font.
pub fn load_default_font() -> Arc<Font> {
    Arc::new(Font::from_slice(DEFAULT_FONT_BYTES).expect("astronomer default font"))
}

/// Trait defining platform-specific capabilities (e.g. Geolocation request on WASM/Native).
pub trait AstronomerPlatform: 'static {
    fn request_geolocation(&self);
}

/// Handles to trigger state updates in the core application from the host platform shells.
pub struct AstronomerHandles {
    pub latitude: Rc<Cell<f64>>,
    pub longitude: Rc<Cell<f64>>,
    pub timestamp_ms: Rc<Cell<i64>>,
    pub yaw: Rc<Cell<f64>>,
    pub pitch: Rc<Cell<f64>>,
    pub roll: Rc<Cell<f64>>,
}

/// Build the shared Instant-Astronomer application.
/// Returns the [`App`] hosting the widget tree and the [`AstronomerHandles`] for platform integration.
pub fn build_astronomer_app<P: AstronomerPlatform>(
    font: Arc<Font>,
    platform: P,
) -> (App, AstronomerHandles) {
    // 1. Core State Cells
    let latitude = Rc::new(Cell::new(39.7392));      // Default: Denver latitude
    let longitude = Rc::new(Cell::new(-104.9903));   // Default: Denver longitude
    let timestamp_ms = Rc::new(Cell::new(web_time::SystemTime::now()
        .duration_since(web_time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64));

    let yaw = Rc::new(Cell::new(0.0));
    let pitch = Rc::new(Cell::new(0.0));
    let roll = Rc::new(Cell::new(0.0));

    let show_constellations = Rc::new(Cell::new(true));

    // Handles to return to the caller
    let handles = AstronomerHandles {
        latitude: Rc::clone(&latitude),
        longitude: Rc::clone(&longitude),
        timestamp_ms: Rc::clone(&timestamp_ms),
        yaw: Rc::clone(&yaw),
        pitch: Rc::clone(&pitch),
        roll: Rc::clone(&roll),
    };

    // 2. Custom Sky and HUD Widgets
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

    let tape_widget = HorizonTapeWidget::new(
        Arc::clone(&font),
        Rc::clone(&yaw),
    );

    // 3. Flat Configuration Control Panel Layout (Bottom Tray)
    // Geolocation Target Button (Label fallback of target icon)
    let platform_arc = Arc::new(platform);
    let platform_clone = Arc::clone(&platform_arc);
    let geo_button = Button::new("📍 Geolocation", Arc::clone(&font))
        .on_click(move || {
            platform_clone.request_geolocation();
        });

    // Constellation lines checkbox
    let constellation_checkbox = Checkbox::new(
        "Constellations",
        Arc::clone(&font),
        show_constellations.get(),
    ).with_state_cell(show_constellations);

    // Location coordinate readout
    let lat_clone = Rc::clone(&latitude);
    let lng_clone = Rc::clone(&longitude);
    let coord_label = DynamicLabel::new(
        move || {
            format!("Lat: {:.4}°  Lng: {:.4}°", lat_clone.get(), lng_clone.get())
        },
        Arc::clone(&font)
    ).with_font_size(11.0);

    // Search input for major cities
    let search_buffer = Rc::new(std::cell::RefCell::new(String::new()));
    let buffer_clone = Rc::clone(&search_buffer);
    let search_field = TextField::new(Arc::clone(&font))
        .with_placeholder("Search city (e.g. London, Tokyo, Paris)...")
        .on_change(move |text| {
            *buffer_clone.borrow_mut() = text.to_string();
        });

    // Search action button
    let lat_search = Rc::clone(&latitude);
    let lng_search = Rc::clone(&longitude);
    let buffer_search = Rc::clone(&search_buffer);
    let search_status = Rc::new(std::cell::RefCell::new(String::from("Ready")));
    let status_label = DynamicLabel::new(
        {
            let status = Rc::clone(&search_status);
            move || status.borrow().clone()
        },
        Arc::clone(&font)
    ).with_font_size(10.0);

    let search_button = Button::new("Search", Arc::clone(&font))
        .on_click(move || {
            let query = buffer_search.borrow();
            let matches = cities::search_cities(&query);
            if let Some(city) = matches.first() {
                lat_search.set(city.latitude);
                lng_search.set(city.longitude);
                *search_status.borrow_mut() = format!("Located: {}, {}", city.name, city.country_code);
            } else {
                *search_status.borrow_mut() = String::from("City not found");
            }
        });

    // Horizontal controls row
    let control_row_1 = FlexRow::new()
        .with_gap(10.0)
        .add(Box::new(geo_button))
        .add(Box::new(constellation_checkbox))
        .add(Box::new(coord_label));

    let control_row_2 = FlexRow::new()
        .with_gap(10.0)
        .add(Box::new(search_field))
        .add(Box::new(search_button))
        .add(Box::new(status_label));

    // Package the Control Panel in a beautiful Container with border + background
    let panel_col = FlexColumn::new()
        .with_gap(8.0)
        .add(Box::new(control_row_1))
        .add(Box::new(control_row_2));

    let panel_container = Container::new()
        .add(Box::new(panel_col))
        .with_background(Color::from_rgb8(28, 28, 40))
        .with_border(Color::from_rgb8(50, 50, 70), 1.0)
        .with_inner_padding(Insets::all(10.0));

    // Assemble the complete viewport layout:
    // - Upper Viewport (SkyViewWidget)
    // - Horizon Tape (HorizonTapeWidget)
    // - Lower Control Panel (Container)
    let root_column = FlexColumn::new()
        .with_gap(0.0)
        .add_flex(Box::new(sky_widget), 1.0)
        .add(Box::new(tape_widget))
        .add(Box::new(panel_container));

    let app = App::new(Box::new(root_column));

    (app, handles)
}
