//! # Native Shell for Instant-Astronomer
//!
//! Thinnest possible desktop shim: everything platform-generic (winit
//! window + event loop, wgpu surface, input forwarding, frame painting,
//! device-loss recovery) lives in the published `agg-gui-shell` crate.
//! This file contributes only what is genuinely specific to
//! Instant-Astronomer on desktop: the [`AstronomerPlatform`]
//! implementation and the per-frame clock tick.

use std::rc::Rc;

use agg_gui_shell::{run, Frame, ShellConfig, ShellError, ShellHost};
use instant_astronomer_core::{
    build_astronomer_app, load_default_font, AstronomerHandles, AstronomerPlatform,
};

/// Desktop implementation of the platform capability surface.
struct NativePlatform;

impl AstronomerPlatform for NativePlatform {
    fn request_geolocation(&self, apply: Rc<dyn Fn(f64, f64)>) {
        // Desktop has no geolocation service; report the Royal Observatory
        // Greenwich so the button still demonstrates the full pipeline.
        eprintln!("Geolocation: no OS location service; using Greenwich Royal Observatory");
        apply(51.4769, 0.0);
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

/// The app-side seam of the shared shell: everything Instant-Astronomer
/// does *around* the shell-owned frame loop, which today is only the
/// projection-clock tick.
struct AstronomerHost {
    handles: AstronomerHandles,
}

impl ShellHost for AstronomerHost {
    // Advance the projection clock every painted frame so celestial
    // bodies animate.
    fn on_frame(&mut self, _app: &mut agg_gui::App, _frame: &Frame) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        self.handles.timestamp_ms.set(now);
    }
}

fn main() -> Result<(), ShellError> {
    run(
        ShellConfig::new("Instant-Astronomer").with_logical_size(1024.0, 768.0),
        |_init| {
            let (app, handles) = build_astronomer_app(load_default_font(), NativePlatform);
            Ok((app, AstronomerHost { handles }))
        },
    )
}
