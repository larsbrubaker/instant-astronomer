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
pub mod search;
pub mod stars;
pub mod toast;

pub mod widgets {
    //! Custom widgets used by the Instant-Astronomer UI shell.
    pub mod horizon_tape;
    pub mod sky_view;
    pub mod status_text;
    pub mod wrapping_row;
}

mod clock;
mod control_panel;
mod fusion;
mod menu;
mod search_panel;

pub use clock::current_unix_ms;
pub use fusion::{
    apply_device_orientation, view_quat_heading_rad, FUSION_TILT_WEIGHT, FUSION_YAW_KNEE_RAD,
};

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widgets::{FlexColumn, Stack};
use agg_gui::App;
use nalgebra::UnitQuaternion;

use crate::control_panel::build_control_panel;
use crate::widgets::horizon_tape::HorizonTapeWidget;
use crate::widgets::sky_view::SkyViewWidget;

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
    /// Trigger a geolocation lookup. When coordinates arrive (possibly
    /// asynchronously), call `apply(latitude_deg, longitude_deg)` — the
    /// core builds `apply` over its own live state cells and it takes
    /// care of the redraw, so implementations never touch app state
    /// directly. (An earlier design asked implementations to update the
    /// handles themselves; both shells ended up writing into private
    /// cells nothing read, so the Locate button silently did nothing.)
    fn request_geolocation(&self, apply: Rc<dyn Fn(f64, f64)>);

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
    /// `true` once the first device-orientation event has fired. The
    /// fusion filter (see [`apply_device_orientation`]) snaps on the
    /// first reading and slerps thereafter — so we know to snap, we
    /// need to remember whether any event has been received.
    pub fusion_seeded: Rc<Cell<bool>>,
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
    // Visibility of the on-screen control chrome (mobile left-edge button
    // rail). Starts hidden for an unobstructed sky on load; a tap on empty
    // sky toggles it (handled in SkyViewWidget). Shared with the control
    // panel, which wraps the rail in a `Conditional` watching this cell.
    let show_controls = Rc::new(Cell::new(false));
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
    let fusion_seeded: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let search_text = Rc::new(std::cell::RefCell::new(String::new()));
    let search_status = Rc::new(std::cell::RefCell::new(String::from("Type a city to search")));

    // Object-search overlay state. `search_active` toggles the top
    // search bar; `search_target` holds the selected object the sky-view
    // finder points at. `search_no_target` is kept as the strict inverse
    // of `search_has_target` so the input-mode `Conditional` (which takes
    // a single bool cell) can show/hide without a derived widget.
    let search_active = Rc::new(Cell::new(false));
    let search_has_target = Rc::new(Cell::new(false));
    let search_no_target = Rc::new(Cell::new(true));
    let search_query = Rc::new(std::cell::RefCell::new(String::new()));
    let search_target: Rc<std::cell::RefCell<Option<crate::search::SearchTarget>>> =
        Rc::new(std::cell::RefCell::new(None));

    let handles = AstronomerHandles {
        latitude: Rc::clone(&latitude),
        longitude: Rc::clone(&longitude),
        timestamp_ms: Rc::clone(&timestamp_ms),
        view_quat: Rc::clone(&view_quat),
        calibration_yaw: Rc::clone(&calibration_yaw),
        use_device_orientation: Rc::clone(&use_device_orientation),
        fusion_seeded: Rc::clone(&fusion_seeded),
    };

    let sky_widget = SkyViewWidget::new(
        Arc::clone(&font),
        Rc::clone(&latitude),
        Rc::clone(&longitude),
        Rc::clone(&timestamp_ms),
        Rc::clone(&view_quat),
        Rc::clone(&calibration_yaw),
        Rc::clone(&show_constellations),
        Rc::clone(&use_device_orientation),
        Rc::clone(&show_controls),
        Rc::clone(&local_offset_fn),
        Rc::clone(&toast),
        Rc::clone(&search_target),
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
        Rc::clone(&show_controls),
        Rc::clone(&use_geolocation),
        Rc::clone(&use_device_orientation),
        Rc::clone(&search_text),
        Rc::clone(&search_status),
        Rc::clone(&search_active),
        Rc::clone(&search_query),
        Rc::clone(&toast),
    );

    let search_panel = crate::search_panel::build_search_panel(
        Arc::clone(&font),
        crate::search_panel::SearchState {
            active: Rc::clone(&search_active),
            has_target: Rc::clone(&search_has_target),
            no_target: Rc::clone(&search_no_target),
            query: Rc::clone(&search_query),
            target: Rc::clone(&search_target),
            timestamp_ms: Rc::clone(&timestamp_ms),
        },
    );

    // Overlay floating chrome on the sky with a `Stack` (`add_aligned`
    // floats each child at its natural size by its own anchor) so the sky
    // shows through everywhere else and pointer events outside the panels
    // still reach the sky. On mobile the control panel also hands back a
    // left-edge button rail; desktop has none. The search bar floats at
    // the top on both form factors.
    let mut stack = Stack::new().add(Box::new(sky_widget));
    if let Some(rail) = panel.left_rail {
        // ReserveInset publishes the rail's strip to agg-gui's overlay
        // insets so floating content (the reticle info card) never
        // renders half-hidden underneath it.
        stack = stack.add_aligned(Box::new(agg_gui::widgets::ReserveInset::left(rail)));
    }
    stack = stack.add_aligned(search_panel);
    let sky_area: Box<dyn agg_gui::widget::Widget> = Box::new(stack);

    let root = FlexColumn::new()
        .with_gap(0.0)
        .add_flex(sky_area, 1.0)
        .add(Box::new(tape_widget))
        .add(Box::new(panel.bottom));

    (App::new(Box::new(root)), handles)
}
#[cfg(test)]
mod overlay_inset_tests {
    use std::rc::Rc;

    use agg_gui::{Modifiers, MouseButton};

    struct NoopPlatform;
    impl crate::AstronomerPlatform for NoopPlatform {
        fn request_geolocation(&self, _apply: Rc<dyn Fn(f64, f64)>) {}
    }

    /// The mobile left rail must reserve its strip with agg-gui's overlay
    /// insets whenever it is visible, so the reticle info card (placed via
    /// `agg_gui::card`) never renders half-hidden underneath it — the
    /// "Algenib card cut off behind the rail" phone bug.
    #[test]
    fn visible_mobile_rail_reserves_left_inset_for_overlays() {
        agg_gui::input_profile::set_input_profile(agg_gui::input_profile::InputProfile::MobileAndroid);
        agg_gui::ux_scale::set_ux_scale(1.7);
        let (mut app, _handles) =
            crate::build_astronomer_app(crate::load_default_font(), NoopPlatform);

        let viewport = agg_gui::geometry::Size::new(390.0 * 1.7, 700.0 * 1.7);
        agg_gui::overlay_insets::begin_frame();
        app.layout(viewport);
        assert_eq!(
            agg_gui::overlay_insets::current().left,
            0.0,
            "rail starts hidden — nothing reserved"
        );

        // Tap empty sky to show the control rail, then re-lay out.
        app.on_mouse_down(40.0, 900.0, MouseButton::Left, Modifiers::default());
        app.on_mouse_up(40.0, 900.0, MouseButton::Left, Modifiers::default());
        agg_gui::overlay_insets::begin_frame();
        app.layout(viewport);

        let left = agg_gui::overlay_insets::current().left;
        assert!(
            (40.0..80.0).contains(&left),
            "visible rail must reserve its strip (got left={left})"
        );

        agg_gui::ux_scale::set_ux_scale(1.0);
        agg_gui::input_profile::set_input_profile(agg_gui::input_profile::InputProfile::Desktop);
    }
}
