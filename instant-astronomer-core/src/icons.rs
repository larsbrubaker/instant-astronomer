//! # Font Awesome Icon Constants
//!
//! Font Awesome 6 Free-Solid code points used by the configuration tray.
//! The TTF lives in `assets/fa.ttf`; load it via [`load_icon_font`] and
//! pass to `Button::with_icon` / `with_icon_sized`.

use agg_gui::text::Font;
use std::sync::Arc;

/// Font Awesome Free-Solid bundled into the binary. Matches the TTF
/// shipped with the Solitaire app (~165 KB).
pub const ICON_FONT_BYTES: &[u8] = include_bytes!("../assets/fa.ttf");

/// Parse the bundled Font Awesome TTF into an `Arc<Font>` usable by
/// `Button::with_icon`. Both shells call this once at startup; the
/// returned `Arc` is cheap to clone per button.
pub fn load_icon_font() -> Arc<Font> {
    Arc::new(Font::from_slice(ICON_FONT_BYTES).expect("instant-astronomer icon font"))
}

/// Crosshairs — used for the "Locate me" geolocation button.
pub const FA_CROSSHAIRS: char = '\u{f05b}';

/// Compass face — used for the "Calibrate to north" button.
pub const FA_COMPASS: char = '\u{f14e}';

/// Expand arrows pointing outward — used for the full-screen toggle.
pub const FA_EXPAND: char = '\u{f065}';

/// Compress arrows pointing inward — used when already full-screen.
pub const FA_COMPRESS: char = '\u{f066}';
