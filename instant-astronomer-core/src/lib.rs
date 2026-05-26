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
pub mod icons;
pub mod math;
pub mod stars;
pub mod toast;

pub mod widgets {
    //! Custom widgets used by the Instant-Astronomer UI shell.
    pub mod horizon_tape;
    pub mod sky_view;
    pub mod status_text;
    pub mod wrapping_row;
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

use crate::icons::{
    load_icon_font, FA_COMPASS, FA_CROSSHAIRS, FA_EXPAND, FA_MAP_MARKER, FA_MOBILE, FA_STAR,
};
use crate::widgets::horizon_tape::HorizonTapeWidget;
use crate::widgets::sky_view::SkyViewWidget;
use crate::widgets::status_text::StatusText;
use crate::widgets::wrapping_row::WrappingRow;

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

    /// Toggle full-screen presentation. WASM calls
    /// `Element.requestFullscreen()` / `Document.exitFullscreen()`;
    /// native shells decide their own meaning (or no-op). Default is
    /// no-op so shells that can't honour this still compile.
    fn toggle_fullscreen(&self) {}
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
    /// Whether to honour device-orientation events. When `false`, the
    /// WASM shell ignores `deviceorientation` callbacks and the user can
    /// swipe to look around. Lets the user opt out when the
    /// magnetometer is mis-calibrated or the phone is on a desk.
    pub use_device_orientation: Rc<Cell<bool>>,
    /// Smoothed compass heading (radians, W3C-CCW-from-north), or
    /// `None` until the first device-orientation event arrives. The
    /// raw magnetometer alpha jitters by several degrees per frame —
    /// especially near horizontal — so we feed it through a heavy
    /// low-pass filter (see [`apply_device_orientation`]) before
    /// composing the view quaternion. Pitch goes through unfiltered
    /// to keep look-up / look-down snappy.
    pub filtered_yaw: Rc<Cell<Option<f64>>>,
}

/// Build the shared Instant-Astronomer widget tree. Both the native and
/// WASM shells call this and forward platform input into the returned
/// [`App`].
pub fn build_astronomer_app<P: AstronomerPlatform>(
    font: Arc<Font>,
    platform: P,
) -> (App, AstronomerHandles) {
    let platform = Rc::new(platform);
    // Closure the SkyView calls every frame to format rise/set in the
    // user's local time. Wraps the platform Rc by clone — the
    // platform owns the OS / browser timezone API.
    let local_offset_fn: Rc<dyn Fn() -> i32> = {
        let p = Rc::clone(&platform);
        Rc::new(move || p.local_offset_minutes())
    };
    // Shared toast cell. Control-panel actions write here; the sky
    // widget paints a transient card. Replaces the explanatory text
    // we used to show alongside the buttons (now icons on mobile).
    let toast = crate::toast::new_toast_cell();
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
    // Honour device-orientation events on mobile (where there's a
    // working compass + gyro), ignore them on desktop (browsers fire
    // events with stale/zero values that would override mouse-drag
    // pans). User can flip the toggle either way.
    let use_device_orientation =
        Rc::new(Cell::new(agg_gui::input_profile::is_mobile_touch()));
    let filtered_yaw: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));
    let search_text = Rc::new(std::cell::RefCell::new(String::new()));
    let search_status = Rc::new(std::cell::RefCell::new(String::from("Type a city to search")));

    let handles = AstronomerHandles {
        latitude: Rc::clone(&latitude),
        longitude: Rc::clone(&longitude),
        timestamp_ms: Rc::clone(&timestamp_ms),
        view_quat: Rc::clone(&view_quat),
        calibration_yaw: Rc::clone(&calibration_yaw),
        use_device_orientation: Rc::clone(&use_device_orientation),
        filtered_yaw: Rc::clone(&filtered_yaw),
    };

    let sky_widget = SkyViewWidget::new(
        Arc::clone(&font),
        Rc::clone(&latitude),
        Rc::clone(&longitude),
        Rc::clone(&timestamp_ms),
        Rc::clone(&view_quat),
        Rc::clone(&calibration_yaw),
        Rc::clone(&show_constellations),
        Rc::clone(&local_offset_fn),
        Rc::clone(&toast),
    );
    let tape_widget = HorizonTapeWidget::new(Arc::clone(&font), Rc::clone(&view_quat));

    let panel = build_control_panel(
        Arc::clone(&font),
        Rc::clone(&platform),
        Rc::clone(&latitude),
        Rc::clone(&longitude),
        Rc::clone(&timestamp_ms),
        Rc::clone(&view_quat),
        Rc::clone(&calibration_yaw),
        Rc::clone(&show_constellations),
        Rc::clone(&use_geolocation),
        Rc::clone(&use_device_orientation),
        Rc::clone(&search_text),
        Rc::clone(&search_status),
        Rc::clone(&toast),
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
    platform: Rc<P>,
    latitude: Rc<Cell<f64>>,
    longitude: Rc<Cell<f64>>,
    timestamp_ms: Rc<Cell<i64>>,
    view_quat: Rc<Cell<UnitQuaternion<f64>>>,
    calibration_yaw: Rc<Cell<f64>>,
    show_constellations: Rc<Cell<bool>>,
    use_geolocation: Rc<Cell<bool>>,
    use_device_orientation: Rc<Cell<bool>>,
    search_text: Rc<std::cell::RefCell<String>>,
    search_status: Rc<std::cell::RefCell<String>>,
    toast: crate::toast::ToastCell,
) -> Container {
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
            b = b.with_compact();
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
            b = b.with_compact();
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
            b = b.with_compact();
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
    // Layout: toggles up front as a single group (subtle/grey when
    // off, accent/blue when on — visually obvious which are active),
    // then the momentary action buttons, then status text.
    let row_1 = WrappingRow::new()
        .with_gap(h_gap, 6.0)
        .add(geo_toggle)
        .add(constellation_toggle)
        .add(compass_toggle)
        .add(Box::new(geo_button))
        .add(Box::new(calibrate_button))
        .add(Box::new(fullscreen_button))
        .add(Box::new(coord_label))
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

/// Low-pass coefficient for the compass heading. Each
/// `deviceorientation` event nudges the filtered yaw by this fraction
/// of the difference, so a sustained input takes ~`1/κ` events to
/// converge (≈ 20 events / ~300 ms at 60 Hz). Heavy by design — the
/// raw magnetometer alpha bounces several degrees per frame on most
/// phones, especially when the device is held near horizontal, and
/// the user reported the view "jerking around" without smoothing.
pub const COMPASS_FILTER_KAPPA: f64 = 0.05;

/// Apply a device-orientation reading to the shared `view_quat`,
/// smoothing the compass heading heavily and passing the gyroscope-
/// derived pitch through unfiltered.
///
/// Inputs are radians: `yaw_rad` is W3C alpha (CCW from north),
/// `pitch_rad` is W3C beta minus 90° (so 0 = looking at horizon).
/// Roll is intentionally dropped — including it adds horizon wobble
/// without giving the user new control.
///
/// The shell calls this on every `deviceorientation` event. The
/// filter state lives in [`AstronomerHandles::filtered_yaw`] so the
/// caller doesn't have to thread it through; first call seeds the
/// filter with the raw value (no startup jerk).
pub fn apply_device_orientation(
    handles: &AstronomerHandles,
    yaw_rad: f64,
    pitch_rad: f64,
) {
    if !handles.use_device_orientation.get() {
        return;
    }
    let next_yaw = match handles.filtered_yaw.get() {
        Some(prev) => crate::math::lerp_angle_rad(prev, yaw_rad, COMPASS_FILTER_KAPPA),
        None => yaw_rad,
    };
    handles.filtered_yaw.set(Some(next_yaw));
    let q_yaw =
        UnitQuaternion::from_axis_angle(&nalgebra::Vector3::y_axis(), next_yaw);
    let q_pitch =
        UnitQuaternion::from_axis_angle(&nalgebra::Vector3::x_axis(), pitch_rad);
    handles.view_quat.set(q_pitch * q_yaw);
    agg_gui::animation::request_draw();
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
