# Instant-Astronomer

Interactive real-time 3D star and celestial sphere overlay in Rust, rendered using **agg-gui**.

This is a Progressive Web App (PWA) and Desktop App built using Rust and WebAssembly, utilizing pure 2D vector graphics drawing primitives to project stars, constellations, planets, the Sun, and the Moon in real-time.

Part of the [rust-apps](https://github.com/larsbrubaker/rust-apps) suite.

## Core Features

- **Heads-Up Display (HUD) Horizon Tape**: Compass directions rolling responsively based on device yaw.
- **Fixed Stars Backdrop**: Calculated using the brightest stars with real astronomical coordinate transformations.
- **Interactive Panning**: Mouse or touch-based dragging to pan around the night sky.
- **Dynamic Solar System Bodies**: Orbit approximations for the Sun, Moon, Mars, and Jupiter.
- **City Search Database**: Soundex phonetic matching and prefix auto-completion.
- **Pure Vector Drawing**: Completely styled and rendered with **agg-gui** (no raw Canvas or WebGL pipeline required).

## Workspace Layout

- `instant-astronomer-core`: Astronomical algorithms, city lookup, and agg-gui UI widgets.
- `instant-astronomer-native`: Thin desktop wrapper (winit + wgpu surface).
- `instant-astronomer-wasm`: Thin WebAssembly wrapper (wasm-bindgen + HTML5 Canvas integration).

## Quick Start

### Native Desktop App

Make sure you have `cargo-watch` installed (`cargo install cargo-watch`), then run:

```bash
cargo dev
```

Or run directly:

```bash
cargo run -p instant-astronomer-native
```
