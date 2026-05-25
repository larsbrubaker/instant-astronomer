//! # Native Shell for Instant-Astronomer
//!
//! This crate is the platform-specific shell for running Instant-Astronomer on desktop
//! operating systems. It initializes an OS window using `winit`, configures a hardware-accelerated
//! 3D rendering context using `wgpu`, and pipes inputs (mouse panning, keyboard, window scaling)
//! into the core immediate-mode UI application in `instant-astronomer-core`.

#![allow(deprecated)] // winit 0.30 EventLoop::run idiom

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{winit_adapter, App, Modifiers, Size};
use demo_wgpu::{begin_frame, WgpuGfxCtx};
use instant_astronomer_core::{
    build_astronomer_app, load_default_font, AstronomerHandles, AstronomerPlatform,
};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes};

/// Struct representing the hardware graphics context (wgpu).
struct Gpu {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,
    config: wgpu::SurfaceConfiguration,
}

impl Gpu {
    fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_desc.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(instance_desc);
        let surface = instance
            .create_surface(window.clone())
            .expect("create wgpu surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("request wgpu adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("astronomer-native-wgpu"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::Off,
        }))
        .expect("request wgpu device");

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
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
            surface,
            surface_format,
            config,
        }
    }

    fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
    }
}

/// Native implementation of the Geolocation service.
struct NativePlatform {
    latitude: Rc<Cell<f64>>,
    longitude: Rc<Cell<f64>>,
}

impl AstronomerPlatform for NativePlatform {
    fn request_geolocation(&self) {
        // Mock geolocation: set coordinates to Royal Observatory Greenwich, London, UK!
        self.latitude.set(51.4769);
        self.longitude.set(0.0000);
        eprintln!("Geolocation: Coordinates updated to Greenwich Royal Observatory (Lat 51.4769, Lng 0.0000)");
        agg_gui::animation::request_draw();
    }

    fn local_offset_minutes(&self) -> i32 {
        // `now_local()` consults the OS time zone (Win32 `GetTimeZoneInformation`
        // on Windows; `/etc/localtime` + tzdata on Unix) and includes DST.
        // Errors mean the platform refused to report a tz — fall back to UTC
        // rather than guess and silently mislead the user.
        time::OffsetDateTime::now_local()
            .map(|d| d.offset().whole_minutes() as i32)
            .unwrap_or(0)
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("create event loop");

    let start_w = 1024;
    let start_h = 768;

    let window_attributes = WindowAttributes::default()
        .with_title("Instant-Astronomer")
        .with_inner_size(LogicalSize::new(start_w, start_h))
        .with_visible(false);
    let window = Arc::new(
        event_loop
            .create_window(window_attributes)
            .expect("create window"),
    );
    agg_gui::set_device_scale(window.scale_factor());

    let mut gpu = Gpu::new(window.clone());

    // Load the default bundled Cascadia font
    let font = load_default_font();

    let lat_cell = Rc::new(Cell::new(39.7392));
    let lng_cell = Rc::new(Cell::new(-104.9903));

    let platform = NativePlatform {
        latitude: Rc::clone(&lat_cell),
        longitude: Rc::clone(&lng_cell),
    };

    let (mut app, handles) = build_astronomer_app(font, platform);

    // Synchronize initial coordinates
    handles.latitude.set(lat_cell.get());
    handles.longitude.set(lng_cell.get());

    let mut wgpu_ctx = WgpuGfxCtx::new(
        Arc::clone(&gpu.device),
        Arc::clone(&gpu.queue),
        gpu.surface_format,
        gpu.config.width as f32,
        gpu.config.height as f32,
    );

    let mut win_w = window.inner_size().width.max(1);
    let mut win_h = window.inner_size().height.max(1);
    let mut cursor_x = 0.0_f64;
    let mut cursor_y = 0.0_f64;
    let mut current_mods = Modifiers::default();
    let mut layout_key: Option<(u32, u32, u64, u64)> = None;
    let mut mouse_buttons_down = 0_u32;

    window.set_visible(true);

    event_loop
        .run(move |event, elwt| match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                elwt.exit();
            }

            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } if size.width > 0 && size.height > 0 => {
                win_w = size.width;
                win_h = size.height;
                gpu.resize(win_w, win_h);
                window.request_redraw();
            }

            Event::WindowEvent {
                event: WindowEvent::ScaleFactorChanged { scale_factor, .. },
                ..
            } => {
                agg_gui::set_device_scale(scale_factor);
                window.request_redraw();
            }

            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, .. },
                ..
            } => {
                cursor_x = position.x;
                cursor_y = position.y;
                app.on_mouse_move(cursor_x, cursor_y);
            }

            Event::WindowEvent {
                event: WindowEvent::ModifiersChanged(mods_state),
                ..
            } => {
                current_mods = winit_adapter::modifiers(mods_state.state());
            }

            Event::WindowEvent {
                event: WindowEvent::MouseInput { state, button, .. },
                ..
            } => {
                let btn = winit_adapter::mouse_button(button);
                match state {
                    ElementState::Pressed => {
                        mouse_buttons_down = mouse_buttons_down.saturating_add(1);
                        app.on_mouse_down(cursor_x, cursor_y, btn, current_mods);
                    }
                    ElementState::Released => {
                        mouse_buttons_down = mouse_buttons_down.saturating_sub(1);
                        app.on_mouse_up(cursor_x, cursor_y, btn, current_mods);
                    }
                }
            }

            Event::WindowEvent {
                event:
                    WindowEvent::MouseWheel {
                        delta: MouseScrollDelta::LineDelta(dx, dy),
                        ..
                    },
                ..
            } => {
                app.on_mouse_wheel_xy_mods(cursor_x, cursor_y, dx as f64, dy as f64, current_mods);
            }

            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        event: key_event, ..
                    },
                ..
            } => {
                let Some(key) = winit_adapter::key_event(&key_event, current_mods) else {
                    return;
                };
                match key_event.state {
                    ElementState::Pressed => {
                        app.on_key_down(key, current_mods);
                    }
                    ElementState::Released => {
                        app.on_key_up(key, current_mods);
                    }
                }
            }

            Event::WindowEvent {
                event: WindowEvent::RedrawRequested,
                ..
            } => {
                paint_frame(
                    &gpu,
                    &mut wgpu_ctx,
                    &mut app,
                    win_w,
                    win_h,
                    &handles,
                    &mut layout_key,
                );
            }

            Event::AboutToWait => {
                if app.wants_draw() {
                    window.request_redraw();
                    elwt.set_control_flow(ControlFlow::Poll);
                } else if let Some(t) = app.next_draw_deadline() {
                    elwt.set_control_flow(ControlFlow::WaitUntil(t));
                } else {
                    elwt.set_control_flow(ControlFlow::Wait);
                }
            }

            _ => {}
        })
        .expect("event loop");
}

fn paint_frame(
    gpu: &Gpu,
    ctx: &mut WgpuGfxCtx,
    app: &mut App,
    win_w: u32,
    win_h: u32,
    handles: &AstronomerHandles,
    layout_key: &mut Option<(u32, u32, u64, u64)>,
) {
    if win_w == 0 || win_h == 0 {
        return;
    }
    let frame = match gpu.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
        _ => return,
    };
    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    // Automatically update timestamp for continuous animation of celestial body movements
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    handles.timestamp_ms.set(now);

    ctx.set_surface_texture(frame.texture.clone());
    ctx.reset(win_w as f32, win_h as f32);
    begin_frame(ctx, view);

    let next_layout_key = (
        win_w,
        win_h,
        agg_gui::device_scale().to_bits(),
        agg_gui::animation::invalidation_epoch(),
    );
    if *layout_key != Some(next_layout_key) {
        app.layout(Size::new(win_w as f64, win_h as f64));
        *layout_key = Some(next_layout_key);
    }

    app.paint(ctx);
    ctx.end_frame();
    frame.present();
}
