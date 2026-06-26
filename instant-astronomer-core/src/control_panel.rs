//! Bottom configuration tray for the Instant-Astronomer UI shell.
//!
//! Extracted from `lib.rs` to keep that file under the workspace
//! line-count guardrail. The control panel is a self-contained group of
//! toggles, action buttons, status readouts, and the city-search row —
//! it talks to the rest of the app only through the shared `Rc<Cell<_>>`
//! state passed in by `build_astronomer_app`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::geometry::Size;
use agg_gui::layout_props::{HAnchor, Insets, VAnchor};
use agg_gui::text::Font;
use agg_gui::widget::Widget;
use agg_gui::widgets::{Button, Checkbox, Conditional, Container, FlexColumn, FlexRow, TextField};
use nalgebra::UnitQuaternion;

/// Edge length (logical px) of every mobile icon button — the left-rail
/// toggles, the top-bar actions, and the three-dot panel entries all use
/// this so they render as a uniform square set.
pub(crate) const ICON_BUTTON_PX: f64 = 32.0;

use crate::clock::format_clock_label;
use crate::fusion::view_quat_heading_rad;
use crate::icons::{
    load_icon_font, FA_COMPASS, FA_CROSSHAIRS, FA_EXPAND, FA_MAP_MARKER, FA_MOBILE, FA_SEARCH,
    FA_STAR,
};
use crate::widgets::status_text::StatusText;
use crate::widgets::wrapping_row::WrappingRow;
use crate::{cities, AstronomerPlatform};

/// The assembled control surface. Desktop puts everything in the bottom
/// tray; mobile pulls the interactive buttons out into a vertical
/// left-edge rail so they're thumb-reachable, leaving only the
/// coordinate / clock readouts (and the conditional city-search row) in
/// the bottom tray.
pub(crate) struct ControlPanel {
    /// Mobile-only vertical button rail anchored to the left edge.
    /// `None` on desktop, where the buttons live in `bottom`.
    pub left_rail: Option<Box<dyn Widget>>,
    /// Bottom tray container (full controls on desktop; readouts +
    /// search on mobile).
    pub bottom: Container,
}

/// Build the configuration controls (geolocation button, constellation
/// toggle, coordinate readout, city search). On mobile the interactive
/// buttons are returned in [`ControlPanel::left_rail`]; on desktop they
/// stay in [`ControlPanel::bottom`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_control_panel<P: AstronomerPlatform>(
    font: Arc<Font>,
    platform: Rc<P>,
    latitude: Rc<Cell<f64>>,
    longitude: Rc<Cell<f64>>,
    timestamp_ms: Rc<Cell<i64>>,
    view_quat: Rc<Cell<UnitQuaternion<f64>>>,
    calibration_yaw: Rc<Cell<f64>>,
    show_constellations: Rc<Cell<bool>>,
    show_controls: Rc<Cell<bool>>,
    use_geolocation: Rc<Cell<bool>>,
    use_device_orientation: Rc<Cell<bool>>,
    search_text: Rc<std::cell::RefCell<String>>,
    search_status: Rc<std::cell::RefCell<String>>,
    search_active: Rc<Cell<bool>>,
    search_query: Rc<RefCell<String>>,
    toast: crate::toast::ToastCell,
) -> ControlPanel {
    let icon_font = load_icon_font();
    // On mobile-touch viewports the action buttons collapse to icon-
    // only so the bottom bar has any chance of fitting on a 400 px
    // wide Pixel screen in portrait. Desktop keeps the text labels —
    // there's plenty of room and the icons alone read as cryptic.
    let mobile = agg_gui::input_profile::is_mobile_touch();

    // Geolocation re-fetch button (works in both modes — even when the
    // user has unchecked "Use geolocation", a quick re-tap fills the
    // city search field with the current location to seed a city
    // lookup).
    let geo_button = {
        let platform = Rc::clone(&platform);
        let toast = Rc::clone(&toast);
        let label = if mobile { "" } else { "Locate me" };
        let mut b = Button::new(label, Arc::clone(&font))
            .with_icon(FA_CROSSHAIRS, Arc::clone(&icon_font))
            .on_click(move || {
                crate::toast::show(&toast, "Locating…");
                platform.request_geolocation();
            });
        if mobile {
            b = b
                .with_compact()
                .with_min_size(Size::new(ICON_BUTTON_PX, ICON_BUTTON_PX));
        }
        b
    };

    // Geolocation toggle. When ON the app uses the device-reported
    // lat/lng; when OFF the city-search field appears. The two are
    // mutually exclusive — geolocation already gives exact lat/lng.
    // `show_search` is the inverted state the `Conditional` wrapping
    // the search row watches; we mirror `use_geolocation` into it on
    // every flip.
    let show_search = Rc::new(Cell::new(!use_geolocation.get()));
    let geo_toggle: Box<dyn agg_gui::widget::Widget> = if mobile {
        let click_cell = Rc::clone(&use_geolocation);
        let active_cell = Rc::clone(&use_geolocation);
        let show_search = Rc::clone(&show_search);
        let toast = Rc::clone(&toast);
        Box::new(
            Button::new("", Arc::clone(&font))
                .with_icon(FA_MAP_MARKER, Arc::clone(&icon_font))
                // `with_subtle` + `with_active_fn` is the segmented
                // toggle look: muted widget_bg (grey) when off, accent
                // (blue) when on. Without `with_subtle` the inactive
                // state is still blue and the user can't tell which
                // toggles are active.
                .with_subtle()
                .with_active_fn(move || active_cell.get())
                .with_compact()
                .with_min_size(Size::new(ICON_BUTTON_PX, ICON_BUTTON_PX))
                .on_click(move || {
                    let new_val = !click_cell.get();
                    click_cell.set(new_val);
                    show_search.set(!new_val);
                    crate::toast::show(
                        &toast,
                        if new_val {
                            "Using device geolocation"
                        } else {
                            "Pick a city to use its coordinates"
                        },
                    );
                    agg_gui::animation::request_draw();
                }),
        )
    } else {
        let show_search = Rc::clone(&show_search);
        let toast = Rc::clone(&toast);
        Box::new(
            Checkbox::new("Use geolocation", Arc::clone(&font), use_geolocation.get())
                .with_state_cell(Rc::clone(&use_geolocation))
                .on_change(move |checked| {
                    show_search.set(!checked);
                    crate::toast::show(
                        &toast,
                        if checked {
                            "Using device geolocation"
                        } else {
                            "Pick a city to use its coordinates"
                        },
                    );
                    agg_gui::animation::request_draw();
                }),
        )
    };

    // Constellation overlay toggle. Mobile uses an icon-only Button
    // with `with_active_fn` so the row stays compact; desktop keeps
    // the labelled Checkbox for clarity. Both write to the same
    // `show_constellations` cell so the rest of the app doesn't care
    // which variant rendered the toggle.
    let constellation_toggle: Box<dyn agg_gui::widget::Widget> = if mobile {
        let click_cell = Rc::clone(&show_constellations);
        let active_cell = Rc::clone(&show_constellations);
        let toast = Rc::clone(&toast);
        Box::new(
            Button::new("", Arc::clone(&font))
                .with_icon(FA_STAR, Arc::clone(&icon_font))
                .with_subtle()
                .with_active_fn(move || active_cell.get())
                .with_compact()
                .with_min_size(Size::new(ICON_BUTTON_PX, ICON_BUTTON_PX))
                .on_click(move || {
                    let new_val = !click_cell.get();
                    click_cell.set(new_val);
                    crate::toast::show(
                        &toast,
                        if new_val {
                            "Constellations on"
                        } else {
                            "Constellations off"
                        },
                    );
                    agg_gui::animation::request_draw();
                }),
        )
    } else {
        let toast = Rc::clone(&toast);
        Box::new(
            Checkbox::new(
                "Constellations",
                Arc::clone(&font),
                show_constellations.get(),
            )
            .with_state_cell(Rc::clone(&show_constellations))
            .on_change(move |checked| {
                crate::toast::show(
                    &toast,
                    if checked { "Constellations on" } else { "Constellations off" },
                );
            }),
        )
    };

    // "Use compass / accel" toggle. When OFF, the WASM shell stops
    // forwarding `deviceorientation` events into `view_quat`, freeing
    // the user to swipe-pan instead — handy when the magnetometer is
    // mis-calibrated or the phone is sitting flat on a desk. Same
    // mobile-icon / desktop-checkbox split as Constellations.
    let compass_toggle: Box<dyn agg_gui::widget::Widget> = if mobile {
        let click_cell = Rc::clone(&use_device_orientation);
        let active_cell = Rc::clone(&use_device_orientation);
        let toast = Rc::clone(&toast);
        Box::new(
            Button::new("", Arc::clone(&font))
                .with_icon(FA_MOBILE, Arc::clone(&icon_font))
                .with_subtle()
                .with_active_fn(move || active_cell.get())
                .with_compact()
                .with_min_size(Size::new(ICON_BUTTON_PX, ICON_BUTTON_PX))
                .on_click(move || {
                    let new_val = !click_cell.get();
                    click_cell.set(new_val);
                    crate::toast::show(
                        &toast,
                        if new_val {
                            "Compass on — orientation sensors driving view"
                        } else {
                            "Compass off — drag to look around"
                        },
                    );
                    agg_gui::animation::request_draw();
                }),
        )
    } else {
        let toast = Rc::clone(&toast);
        Box::new(
            Checkbox::new(
                "Use compass",
                Arc::clone(&font),
                use_device_orientation.get(),
            )
            .with_state_cell(Rc::clone(&use_device_orientation))
            .on_change(move |checked| {
                crate::toast::show(
                    &toast,
                    if checked {
                        "Compass on — sensors driving view"
                    } else {
                        "Compass off — drag to look around"
                    },
                );
            }),
        )
    };

    // Calibrate-to-north button: snapshots the current compass heading
    // derived from `view_quat` into `calibration_yaw`. The projection
    // subtracts this offset on every frame, so the direction the
    // user's phone is currently pointing becomes the rendered
    // "north". A second tap somewhere else re-snaps.
    let calibrate_button = {
        let vq = Rc::clone(&view_quat);
        let cal = Rc::clone(&calibration_yaw);
        let toast = Rc::clone(&toast);
        let label = if mobile { "" } else { "Calibrate" };
        let mut b = Button::new(label, Arc::clone(&font))
            .with_icon(FA_COMPASS, Arc::clone(&icon_font))
            .on_click(move || {
                cal.set(view_quat_heading_rad(vq.get()));
                crate::toast::show(&toast, "Calibrated to current heading");
                agg_gui::animation::request_draw();
            });
        if mobile {
            b = b
                .with_compact()
                .with_min_size(Size::new(ICON_BUTTON_PX, ICON_BUTTON_PX));
        }
        b
    };

    // Full-screen toggle. Icon-only (no label) in both modes — the
    // four-arrow expand glyph is universally recognised. The platform
    // shell decides what "fullscreen" means: WASM calls the browser
    // Fullscreen API; native is a no-op today.
    let fullscreen_button = {
        let platform = Rc::clone(&platform);
        let toast = Rc::clone(&toast);
        let mut b = Button::new("", Arc::clone(&font))
            .with_icon(FA_EXPAND, Arc::clone(&icon_font))
            .on_click(move || {
                platform.toggle_fullscreen();
                crate::toast::show(&toast, "Toggled fullscreen");
            });
        if mobile {
            b = b
                .with_compact()
                .with_min_size(Size::new(ICON_BUTTON_PX, ICON_BUTTON_PX));
        }
        b
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

    // WrappingRow instead of FlexRow so the bottom bar flows onto a
    // second row when it can't fit (e.g. Pixel in portrait). On wider
    // viewports it stays a single row — no visual change for desktop /
    // landscape tablets.
    // Tighter gap on mobile — the buttons themselves shrink via
    // `with_compact()`, so packing them closer keeps the row from
    // wrapping for a few more pixels of viewport width.
    let h_gap = if mobile { 6.0 } else { 12.0 };
    // Layout differs by form factor:
    //   * Desktop — one bottom `WrappingRow`: toggles, then the
    //     momentary action buttons, then the status readouts.
    //   * Mobile — the six interactive buttons move into a vertical
    //     left-edge rail (`left_rail`), so the bottom tray keeps just
    //     the coordinate / clock readouts (plus the conditional search
    //     row built below).
    // Toggles are subtle/grey when off and accent/blue when on, so the
    // active state is obvious whichever layout renders them.
    #[allow(clippy::type_complexity)]
    let (left_rail, row_1): (Option<Box<dyn Widget>>, Box<dyn Widget>) = if mobile {
        // Search at the top, three-dot "more options" at the bottom; the
        // kebab's flyout (Locate me, Calibrate) pops out to the right of
        // the rail, bottom-aligned with the kebab.
        let rail_menu = crate::menu::build_rail_menu(
            Arc::clone(&font),
            Arc::clone(&icon_font),
            vec![Box::new(geo_button), Box::new(calibrate_button)],
            Rc::clone(&search_active),
            Rc::clone(&search_query),
        );

        // The button column floats over the sky as a `Stack` overlay
        // (see lib.rs). `fit_width` keeps it as narrow as the widest
        // button. Its only background is the small black panel hugging
        // the buttons — the sky shows through everywhere else.
        let button_group = FlexColumn::new()
            .with_gap(8.0)
            .with_inner_padding(Insets::all(6.0))
            // Same backing as the altitude-scale HUD on the right:
            // pure black at alpha 110 so the two overlays read with
            // identical see-through over the sky.
            .with_background(Color::from_rgba8(0, 0, 0, 110))
            .with_fit_width(true)
            // `geo_toggle` is intentionally not added here (hidden from the
            // mobile rail for now; the JS shell auto-requests geolocation on
            // page load). The manual "Locate me" button (`geo_button`) and
            // "Calibrate" now live in the three-dot flyout below.
            .add(rail_menu.search_button)
            .add(constellation_toggle)
            .add(compass_toggle)
            .add(Box::new(fullscreen_button))
            .add(rail_menu.menu_button);
        // Pair the button column with the kebab flyout in a content-fit row
        // so the flyout floats to the column's right without resizing it —
        // opening the flyout never shifts the centred rail.
        let rail_row = FlexRow::new()
            .with_gap(8.0)
            .with_fit_width(true)
            .add(Box::new(button_group))
            .add(rail_menu.flyout);
        // Wrap in a `Conditional` so a tap on empty sky (handled in
        // SkyViewWidget) can hide / show the whole rail via
        // `show_controls`. The LEFT + CENTER anchors live on the
        // `Conditional` because the `Stack` overlay aligns whatever
        // widget it's handed directly, pinning the rail flush to the left
        // edge, vertically centred.
        let rail = Conditional::new(Rc::clone(&show_controls), Box::new(rail_row))
            .with_h_anchor(HAnchor::LEFT)
            .with_v_anchor(VAnchor::CENTER);
        let readouts = WrappingRow::new()
            .with_gap(h_gap, 6.0)
            .add(Box::new(coord_label))
            .add(Box::new(time_label));
        (Some(Box::new(rail)), Box::new(readouts))
    } else {
        // Desktop entry point for object search. Labelled "Find" to
        // distinguish it from the city-location "Search" field below.
        let find_button = {
            let active = Rc::clone(&search_active);
            let query = Rc::clone(&search_query);
            Button::new("Find", Arc::clone(&font))
                .with_icon(FA_SEARCH, Arc::clone(&icon_font))
                .on_click(move || {
                    query.borrow_mut().clear();
                    active.set(true);
                    agg_gui::focus::request_focus(crate::search_panel::SEARCH_FIELD_FOCUS_ID);
                    agg_gui::animation::request_draw();
                })
        };
        let row = WrappingRow::new()
            .with_gap(h_gap, 6.0)
            .add(geo_toggle)
            .add(constellation_toggle)
            .add(compass_toggle)
            .add(Box::new(geo_button))
            .add(Box::new(calibrate_button))
            .add(Box::new(find_button))
            .add(Box::new(fullscreen_button))
            .add(Box::new(coord_label))
            .add(Box::new(time_label));
        (None, Box::new(row))
    };

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
        .add(row_1)
        .add(Box::new(row_2_conditional));

    let bottom = Container::new()
        .add(Box::new(inner))
        .with_fit_height(true)
        .with_background(Color::from_rgb8(28, 28, 40))
        .with_border(Color::from_rgb8(50, 50, 70), 1.0)
        .with_inner_padding(Insets::all(12.0));

    ControlPanel { left_rail, bottom }
}
