//! # WebAssembly Shell for Instant-Astronomer
//!
//! Thinnest possible browser shim: everything platform-generic (canvas
//! sizing, wgpu/WebGL2 surface, the rAF loop, DOM pointer / wheel /
//! keyboard / clipboard listeners, DPR + client-platform detection) lives
//! in `demo_wgpu::web_shell`. This crate contributes only what is
//! genuinely specific to Instant-Astronomer in a browser:
//!
//! - the [`AstronomerPlatform`] implementation (navigator.geolocation,
//!   timezone offset, fullscreen toggle),
//! - `#[wasm_bindgen]` exports for the sensor/geolocation results the JS
//!   bootstrap forwards (device orientation needs a JS-side iOS
//!   permission gate; geolocation is auto-requested at page load),
//! - the per-frame projection-clock tick.

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use demo_wgpu::web_shell;
use instant_astronomer_core::{
    build_astronomer_app, load_default_font, AstronomerHandles, AstronomerPlatform,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

thread_local! {
    /// Live state cells shared with the core app — written by the sensor
    /// and geolocation exports below.
    static HANDLES: RefCell<Option<AstronomerHandles>> = const { RefCell::new(None) };
}

/// Browser implementation of the platform capability surface.
struct WasmPlatform;

impl AstronomerPlatform for WasmPlatform {
    fn local_offset_minutes(&self) -> i32 {
        // `Date.getTimezoneOffset()` returns minutes WEST of UTC with
        // DST applied (e.g. PDT → +420 in JS). The trait wants east-
        // positive minutes (e.g. PDT → -420), so negate.
        -(js_sys::Date::new_0().get_timezone_offset() as i32)
    }

    fn toggle_fullscreen(&self) {
        // Delegate to the document — the browser knows what element
        // wraps the canvas and fires its own `fullscreenchange`.
        let _ = js_sys::eval(
            "if (document.fullscreenElement) { document.exitFullscreen(); } \
             else { document.documentElement.requestFullscreen(); }",
        );
    }

    fn request_geolocation(&self, apply: Rc<dyn Fn(f64, f64)>) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let Ok(geolocation) = window.navigator().geolocation() else {
            web_sys::console::error_1(&JsValue::from_str(
                "navigator.geolocation unavailable (insecure context?)",
            ));
            return;
        };

        let success: Closure<dyn FnMut(web_sys::Position)> =
            Closure::new(move |pos: web_sys::Position| {
                let coords = pos.coords();
                apply(coords.latitude(), coords.longitude());
                web_shell::mark_dirty();
            });
        let error: Closure<dyn FnMut(web_sys::PositionError)> =
            Closure::new(move |err: web_sys::PositionError| {
                web_sys::console::error_1(&JsValue::from_str(&format!(
                    "geolocation error code={} message={}",
                    err.code(),
                    err.message()
                )));
            });

        let _ = geolocation.get_current_position_with_error_callback(
            success.as_ref().unchecked_ref(),
            Some(error.as_ref().unchecked_ref()),
        );

        // Leak so the browser can invoke whichever callback fires.
        success.forget();
        error.forget();
    }
}

#[wasm_bindgen(start)]
pub fn start() {
    web_shell::start(
        "astronomer-canvas",
        || {
            let (app, handles) = build_astronomer_app(load_default_font(), WasmPlatform);
            HANDLES.with(|h| *h.borrow_mut() = Some(handles));
            app
        },
        // Advance the projection clock every tick and keep the loop hot —
        // celestial-body positions depend on wall time, so the sky must
        // repaint continuously.
        || {
            HANDLES.with(|h| {
                if let Some(h) = h.borrow().as_ref() {
                    h.timestamp_ms.set(js_sys::Date::now() as i64);
                }
            });
            web_shell::mark_dirty();
        },
    );
}

/// Push a compass + tilt reading from the browser's `deviceorientation`
/// (or `deviceorientationabsolute`) event into the core's `view_quat`
/// state cell.
///
/// All three W3C Euler angles are forwarded unchanged (just converted to
/// radians) — the core consumes them as a continuous rotation matrix, so
/// it stays well-behaved across the `β = π/2` pole. The JS bootstrap owns
/// the event subscription because iOS requires a user-gesture permission
/// prompt (a DOM concern) before events flow:
///
/// - `alpha_deg`: W3C alpha — CCW from magnetic north. JS hands in
///   `event.alpha` on Android-absolute or `360 - webkitCompassHeading`
///   on iOS so the value is always W3C-CCW.
/// - `beta_deg`: front-to-back tilt. 0 = flat face-up; 90 = upright.
/// - `gamma_deg`: left-to-right tilt (the roll signal).
#[wasm_bindgen]
pub fn on_device_orientation(alpha_deg: f64, beta_deg: f64, gamma_deg: f64) {
    HANDLES.with(|h_cell| {
        if let Some(h) = h_cell.borrow().as_ref() {
            instant_astronomer_core::apply_device_orientation(
                h,
                alpha_deg.to_radians(),
                beta_deg.to_radians(),
                gamma_deg.to_radians(),
            );
        }
    });
    web_shell::mark_dirty();
}

/// Set the user's latitude / longitude in **degrees** directly from the
/// JS bootstrap (the page-load `navigator.geolocation` auto-request).
#[wasm_bindgen]
pub fn set_location_degrees(latitude_deg: f64, longitude_deg: f64) {
    HANDLES.with(|h_cell| {
        if let Some(h) = h_cell.borrow().as_ref() {
            h.latitude.set(latitude_deg);
            h.longitude.set(longitude_deg);
        }
    });
    agg_gui::animation::request_draw();
    web_shell::mark_dirty();
}
