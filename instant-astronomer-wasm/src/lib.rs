//! # WebAssembly Shell for Instant-Astronomer
//!
//! This crate is the WebAssembly shell compiled for `wasm32-unknown-unknown`.
//! It exposes the entrypoints to the browser, binds to the HTML5 `<canvas>`,
//! handles DOM pointer, wheel, and keyboard inputs, and integrates with the browser's native
//! `navigator.geolocation` API to determine the user's coordinates.

#![cfg(target_arch = "wasm32")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{App, Modifiers, MouseButton, Size};
use demo_wgpu::{begin_frame, WgpuGfxCtx};
use instant_astronomer_core::{
    build_astronomer_app, load_default_font, AstronomerHandles, AstronomerPlatform,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static HANDLES: RefCell<Option<AstronomerHandles>> = const { RefCell::new(None) };
    static WGPU_INIT: RefCell<Option<WgpuInit>> = const { RefCell::new(None) };
    static WGPU_CTX: RefCell<Option<WgpuGfxCtx>> = const { RefCell::new(None) };
    static NEEDS_DRAW: Cell<bool> = const { Cell::new(true) };
}

struct WgpuInit {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,
    config: wgpu::SurfaceConfiguration,
}

/// WebAssembly implementation of the AstronomerPlatform.
struct WasmPlatform {
    latitude: Rc<Cell<f64>>,
    longitude: Rc<Cell<f64>>,
}

impl AstronomerPlatform for WasmPlatform {
    fn local_offset_minutes(&self) -> i32 {
        // `Date.getTimezoneOffset()` returns minutes WEST of UTC with
        // DST applied (e.g. PDT → +420 in JS). The trait wants east-
        // positive minutes (e.g. PDT → -420), so negate.
        -(js_sys::Date::new_0().get_timezone_offset() as i32)
    }

    fn toggle_fullscreen(&self) {
        // Delegate to the JS shell — that side knows what element
        // wraps the canvas + can wire its own `fullscreenchange`
        // listener for repaints. We just call the exported helper.
        let _ = js_sys::eval(
            "if (document.fullscreenElement) { document.exitFullscreen(); } \
             else { document.documentElement.requestFullscreen(); }",
        );
    }

    fn request_geolocation(&self) {
        let Some(window) = web_sys::window() else {
            return;
        };
        // Convert latitude / longitude in **radians** internally — coords()
        // hands us degrees, so convert at the boundary.
        let Ok(geolocation) = window.navigator().geolocation() else {
            web_sys::console::error_1(&JsValue::from_str(
                "navigator.geolocation unavailable (insecure context?)",
            ));
            return;
        };

        let lat_cell = Rc::clone(&self.latitude);
        let lng_cell = Rc::clone(&self.longitude);

        let success: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Position)> =
            wasm_bindgen::closure::Closure::new(move |pos: web_sys::Position| {
                let coords = pos.coords();
                // State cells store degrees (user-facing); math boundary
                // converts to radians.
                lat_cell.set(coords.latitude());
                lng_cell.set(coords.longitude());
                web_sys::console::log_1(&JsValue::from_str(&format!(
                    "geolocation: lat {} lng {}",
                    coords.latitude(),
                    coords.longitude()
                )));
                agg_gui::animation::request_draw();
                mark_dirty();
            });

        let error: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::PositionError)> =
            wasm_bindgen::closure::Closure::new(move |err: web_sys::PositionError| {
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

        // Leak to let the browser invoke whichever callback fires.
        success.forget();
        error.forget();
    }
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    // Don't build the widget tree here. `draw_frame()` builds it
    // lazily via `ensure_app()`, which runs AFTER the JS shell has
    // called `set_client_platform(...)` — so `is_mobile_touch()`
    // returns the right answer when the control panel decides
    // between icon-only and labelled toggles. Building eagerly here
    // would freeze the layout in Desktop mode on a real phone.

    // Spawn async initialization of wgpu on the browser canvas
    wasm_bindgen_futures::spawn_local(async {
        match init_wgpu_async().await {
            Ok(init) => {
                WGPU_INIT.with(|c| *c.borrow_mut() = Some(init));
            }
            Err(err) => {
                web_sys::console::error_1(&JsValue::from_str(&format!("WASM wgpu init failed: {err}")));
            }
        }
        mark_dirty();
    });
}

#[derive(Debug)]
struct WebDisplay;

impl wgpu::rwh::HasDisplayHandle for WebDisplay {
    fn display_handle(&self) -> Result<wgpu::rwh::DisplayHandle<'_>, wgpu::rwh::HandleError> {
        Ok(wgpu::rwh::DisplayHandle::web())
    }
}

async fn init_wgpu_async() -> Result<WgpuInit, String> {
    let document = web_sys::window()
        .ok_or_else(|| "no global window".to_string())?
        .document()
        .ok_or_else(|| "no document".to_string())?;
    let canvas = document
        .get_element_by_id("astronomer-canvas")
        .ok_or_else(|| "#astronomer-canvas element not found".to_string())?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| "#astronomer-canvas is not a canvas element".to_string())?;

    let mut instance_desc = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(WebDisplay));
    instance_desc.backends = wgpu::Backends::GL;
    let instance = wgpu::Instance::new(instance_desc);
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .map_err(|err| format!("create_surface: {err:?}"))?;

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .map_err(|err| format!("request_adapter: {err:?}"))?;

    let adapter_limits = adapter.limits();
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("astronomer-wasm-wgpu"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter_limits),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(|err| format!("request_device: {err:?}"))?;

    let caps = surface.get_capabilities(&adapter);
    let surface_format = caps
        .formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .unwrap_or(caps.formats[0]);

    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width: canvas.width().max(1),
        height: canvas.height().max(1),
        present_mode: wgpu::PresentMode::AutoVsync,
        desired_maximum_frame_latency: 2,
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
    };
    surface.configure(&device, &config);

    Ok(WgpuInit {
        device: Arc::new(device),
        queue: Arc::new(queue),
        surface,
        surface_format,
        config,
    })
}

fn ensure_app() {
    APP.with(|cell| {
        if cell.borrow().is_some() {
            return;
        }
        let font = load_default_font();
        let lat_cell = Rc::new(Cell::new(39.7392));
        let lng_cell = Rc::new(Cell::new(-104.9903));

        let platform = WasmPlatform {
            latitude: Rc::clone(&lat_cell),
            longitude: Rc::clone(&lng_cell),
        };

        let (app, handles) = build_astronomer_app(font, platform);
        handles.latitude.set(lat_cell.get());
        handles.longitude.set(lng_cell.get());

        HANDLES.with(|h| *h.borrow_mut() = Some(handles));
        *cell.borrow_mut() = Some(app);
    });
}

fn ensure_wgpu_ctx(width: f32, height: f32) {
    WGPU_CTX.with(|ctx_cell| {
        if ctx_cell.borrow().is_some() {
            return;
        }
        WGPU_INIT.with(|init_cell| {
            let init = init_cell.borrow();
            let Some(init) = init.as_ref() else {
                return;
            };
            let ctx = WgpuGfxCtx::new(
                Arc::clone(&init.device),
                Arc::clone(&init.queue),
                init.surface_format,
                width,
                height,
            );
            *ctx_cell.borrow_mut() = Some(ctx);
        });
    });
}

#[wasm_bindgen]
pub fn mark_dirty() {
    NEEDS_DRAW.set(true);
}

#[wasm_bindgen]
pub fn draw_frame() -> bool {
    ensure_app();

    // Grab current canvas size
    let window = match web_sys::window() {
        Some(w) => w,
        None => return false,
    };
    let document = match window.document() {
        Some(d) => d,
        None => return false,
    };
    let canvas = match document.get_element_by_id("astronomer-canvas") {
        Some(el) => match el.dyn_into::<web_sys::HtmlCanvasElement>() {
            Ok(c) => c,
            Err(_) => return false,
        },
        None => return false,
    };

    let w = canvas.width();
    let h = canvas.height();
    if w == 0 || h == 0 {
        return false;
    }

    ensure_wgpu_ctx(w as f32, h as f32);

    WGPU_INIT.with(|init_cell| {
        let mut init_mut = init_cell.borrow_mut();
        let Some(init) = init_mut.as_mut() else {
            return;
        };

        // Resize surface configuration if canvas dimensions changed
        if init.config.width != w || init.config.height != h {
            init.config.width = w;
            init.config.height = h;
            init.surface.configure(&init.device, &init.config);
            WGPU_CTX.with(|ctx_cell| {
                if let Some(ctx) = ctx_cell.borrow_mut().as_mut() {
                    ctx.reset(w as f32, h as f32);
                }
            });
        }
    });

    let frame = WGPU_INIT.with(|init_cell| {
        let init = init_cell.borrow();
        let init = init.as_ref()?;
        match init.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => Some(f),
            _ => None,
        }
    });

    let Some(frame) = frame else {
        return false;
    };

    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    // Update timestamp continuously so celestial-body positions animate.
    // The JS shell can override this between frames via `set_timestamp_ms`
    // to lock the projection to a specific moment (e.g. "show me what the
    // sky will look like tonight at 9pm").
    HANDLES.with(|h_cell| {
        if let Some(h) = h_cell.borrow().as_ref() {
            let now = web_time::SystemTime::now()
                .duration_since(web_time::SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            h.timestamp_ms.set(now);
        }
    });

    APP.with(|app_cell| {
        if let Some(app) = app_cell.borrow_mut().as_mut() {
            app.layout(Size::new(w as f64, h as f64));

            WGPU_CTX.with(|ctx_cell| {
                if let Some(ctx) = ctx_cell.borrow_mut().as_mut() {
                    ctx.set_surface_texture(frame.texture.clone());
                    ctx.reset(w as f32, h as f32);
                    begin_frame(ctx, view);
                    app.paint(ctx);
                    ctx.end_frame();
                }
            });
        }
    });

    frame.present();
    NEEDS_DRAW.set(false);

    // Request another draw if animations are running
    APP.with(|app_cell| {
        if let Some(app) = app_cell.borrow().as_ref() {
            app.wants_draw()
        } else {
            false
        }
    })
}

// Event routing from JavaScript frontend
#[wasm_bindgen]
pub fn on_mouse_move(x: f64, y: f64) {
    APP.with(|app_cell| {
        if let Some(app) = app_cell.borrow_mut().as_mut() {
            app.on_mouse_move(x, y);
        }
    });
}

#[wasm_bindgen]
pub fn on_mouse_down(x: f64, y: f64, button: u8, shift: bool, ctrl: bool, alt: bool, meta: bool) {
    APP.with(|app_cell| {
        if let Some(app) = app_cell.borrow_mut().as_mut() {
            let btn = match button {
                0 => MouseButton::Left,
                1 => MouseButton::Middle,
                2 => MouseButton::Right,
                b => MouseButton::Other(b),
            };
            let mods = Modifiers { shift, ctrl, alt, meta };
            app.on_mouse_down(x, y, btn, mods);
        }
    });
}

#[wasm_bindgen]
pub fn on_mouse_up(x: f64, y: f64, button: u8, shift: bool, ctrl: bool, alt: bool, meta: bool) {
    APP.with(|app_cell| {
        if let Some(app) = app_cell.borrow_mut().as_mut() {
            let btn = match button {
                0 => MouseButton::Left,
                1 => MouseButton::Middle,
                2 => MouseButton::Right,
                b => MouseButton::Other(b),
            };
            let mods = Modifiers { shift, ctrl, alt, meta };
            app.on_mouse_up(x, y, btn, mods);
        }
    });
}

/// Push a smoothed compass + tilt reading from the browser's
/// `deviceorientation` (or `deviceorientationabsolute`) event.
///
/// Inputs are in **degrees**, matching the browser API. The JS shell is
/// responsible for picking the right "alpha" value:
/// - On iOS, `event.webkitCompassHeading` (clockwise from north) is what
///   you want — pass `360 - heading` so we end up with the counter-
///   clockwise yaw the rotation matrix expects.
/// - On Android Chrome with `deviceorientationabsolute`, pass `event.alpha`
///   directly.
/// Hand the device-pixel ratio to agg-gui so it can scale text /
/// strokes / UI by the right factor.
///
/// **This is required for the app to look right on HiDPI mobile
/// screens.** Without it, agg-gui treats the canvas dimensions as
/// logical pixels — so on a Pixel 8 with `devicePixelRatio = 3`,
/// every glyph renders 3× too small to read.
///
/// Call once at boot with `window.devicePixelRatio`, and again from a
/// resize listener so a CSS-zoom or window-resize that changes the DPR
/// (e.g. dragging a window from one monitor to another) keeps the
/// rendering crisp.
#[wasm_bindgen]
pub fn set_device_pixel_ratio(dpr: f64) {
    agg_gui::set_device_scale(dpr.max(0.5));
    mark_dirty();
}

/// Hand the JS-side platform name + `(pointer: coarse)` detection result
/// to agg-gui so:
///
/// - shortcut labels (Cmd vs. Ctrl) match the user's actual OS,
/// - the agg-gui on-screen keyboard auto-enables on mobile touch devices
///   (instead of the awful native browser keyboard).
///
/// Idempotent — call again if the host wants to refresh after viewport
/// rotation / window resize.
#[wasm_bindgen]
pub fn set_client_platform(name: &str, pointer_coarse: bool) {
    agg_gui::set_platform(agg_gui::platform_from_name(name));
    let profile = agg_gui::input_profile::input_profile_from_hint(name, pointer_coarse);
    agg_gui::input_profile::set_input_profile(profile);
    agg_gui::widgets::on_screen_keyboard::set_enabled(profile.is_mobile_touch());
    // Apply the recommended UX zoom only here — at the **platform
    // shell** boundary where we actually know the user is on a real
    // touch device. `set_input_profile` deliberately doesn't, so
    // programmatic profile changes (e.g. agg-gui's mobile-keyboard
    // demo's radio) don't silently resize the desktop UI.
    agg_gui::ux_scale::set_ux_scale(profile.recommended_ux_scale());
    mark_dirty();
}

/// `true` while the agg-gui on-screen keyboard panel is visible. The JS
/// shell uses this to suppress any of its own native-keyboard
/// workarounds (we don't have one yet for instant-astronomer, but the
/// export is kept for parity with agg-gui's `demo-wasm`).
#[wasm_bindgen]
pub fn software_keyboard_visible() -> bool {
    agg_gui::widgets::on_screen_keyboard::is_visible()
}

/// Push a compass + tilt reading from the browser's
/// `deviceorientation` (or `deviceorientationabsolute`) event into the
/// core's `view_quat` state cell.
///
/// Conversions:
/// - `alpha_deg`: W3C alpha — CCW from magnetic north. JS hands in
///   `event.alpha` on Android-absolute or `360 - webkitCompassHeading`
///   on iOS so the value is always W3C-CCW.
/// - `beta_deg`: front-to-back tilt. 0 = flat face-up; 90 = upright;
///   180 = face-down. The projection wants `pitch = 0` for "looking
///   at the horizon", so we subtract 90.
/// - `gamma_deg`: left-to-right tilt. Ignored for now (gives a stable
///   horizon even if the user holds the phone slightly rolled); the
///   value's still part of the conversion so adding roll support is
///   just a one-line change.
///
/// The three Euler angles are composed into a unit quaternion as
/// `Rx(pitch) * Ry(yaw)` -- world→view -- which is what the
/// sky-view projection consumes. No gimbal lock at the zenith.
#[wasm_bindgen]
pub fn on_device_orientation(alpha_deg: f64, beta_deg: f64, gamma_deg: f64) {
    let _ = gamma_deg; // roll wired but unused
    let yaw = alpha_deg.to_radians();
    let pitch = (beta_deg - 90.0).to_radians();
    // `apply_device_orientation` does the heavy-yaw / responsive-pitch
    // smoothing and honours the "Use compass" toggle.
    HANDLES.with(|h_cell| {
        if let Some(h) = h_cell.borrow().as_ref() {
            instant_astronomer_core::apply_device_orientation(h, yaw, pitch);
        }
    });
    mark_dirty();
}

/// Set the user's latitude / longitude in **degrees** directly from the
/// JS shell (e.g. after `navigator.geolocation.getCurrentPosition` resolves
/// or a user picks a location in an OS picker).
#[wasm_bindgen]
pub fn set_location_degrees(latitude_deg: f64, longitude_deg: f64) {
    HANDLES.with(|h_cell| {
        if let Some(h) = h_cell.borrow().as_ref() {
            h.latitude.set(latitude_deg);
            h.longitude.set(longitude_deg);
        }
    });
    agg_gui::animation::request_draw();
    mark_dirty();
}

/// Override the projection clock — useful for "what will the sky look like
/// at sunset" simulations or for unit testing against a fixed timestamp.
/// Passing `0` reverts to whatever the next frame's wall-clock reading is.
#[wasm_bindgen]
pub fn set_timestamp_ms(timestamp_ms: f64) {
    HANDLES.with(|h_cell| {
        if let Some(h) = h_cell.borrow().as_ref() {
            h.timestamp_ms.set(timestamp_ms as i64);
        }
    });
    agg_gui::animation::request_draw();
    mark_dirty();
}

/// Returns `true` if the app currently wants another frame painted.
/// Lets the JS rAF loop go idle when nothing is animating.
#[wasm_bindgen]
pub fn wants_draw() -> bool {
    APP.with(|app_cell| {
        app_cell
            .borrow()
            .as_ref()
            .map(|app| app.wants_draw())
            .unwrap_or(false)
    })
}
