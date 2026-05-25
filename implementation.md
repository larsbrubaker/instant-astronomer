# Instant-Astronomer.com — Implementation Specification

This document details the architectural design and implementation roadmap for **Instant-Astronomer.com**, a lightweight, serverless, client-side progressive web application. The app allows users to input their location (via manual entry or geolocation) and point their device at the night sky to view an interactive, real-time 3D overlay of stars, planets, the Sun, the Moon, and constellations.

The entire user interface — controls, panels, overlays, and the embedded 3D sky viewport — is built on **[`agg-gui`](https://github.com/larsbrubaker/agg-gui)**, an immediate-mode Rust GUI library with a unified widget set, theming system, and platform adapters that run identically on native desktop and in the browser via WASM.

---

## 1. System Architecture Overview

The system is designed to run entirely client-side inside a WebAssembly (WASM) runtime compiled from Rust, leveraging the device's native sensors and hardware-accelerated graphics via `wgpu`.

```
                  ┌────────────────────────────────────────┐
                  │          Browser (Client Side)         │
                  │                                        │
                  │  ┌───────────────┐   ┌──────────────┐  │
  ┌────────────┐  │  │  Asset Fetch  │──>│ Decompress   │  │
  │  GitHub    │──┼─>│  (CSV / DB)   │   │ (Native/WASM)│  │
  │  Pages     │  │  │  (CSV / DB)   │   │ (Native/WASM)│  │
  └────────────┘  │  └───────────────┘   └──────┬───────┘  │
                  │                             │          │
                  │                             ▼          │
                  │                      ┌──────────────┐  │
                  │                      │ SQLite Memory│  │
                  │                      │  (FTS5 /     │  │
                  │                      │  Soundex)    │  │
                  │                      └──────┬───────┘  │
                  │                             │          │
                  │                             ▼          │
  ┌────────────┐  │  ┌───────────────┐   ┌──────────────┐  │
  │ Device     │──┼─>│ Telemetry     │──>│ Rust Engine  │  │
  │  Sensors    │  │  │ (GPS/IMU/Time)│   │ Matrix Math  │  │
  └────────────┘  │  └───────────────┘   └──────┬───────┘  │
                  │                             │          │
                  │                             ▼          │
                  │                      ┌──────────────┐  │
                  │                      │ wgpu Pipeline│  │
                  │                      │ (3D Render)  │  │
                  │                      └──────────────┘  │
                  └────────────────────────────────────────┘
```

### Core Architecture Principles
* **Zero Backend Costs:** All data assets are hosted statically on GitHub Pages as compressed payloads.
* **Low Memory Footprint:** Data is parsed streaming or queried via an in-memory SQLite instances to prevent DOM memory exhaustion.
* **Sub-Millisecond Engine Interfacing:** All projection, coordinate transformations, and matrix transformations are performed in native Rust via WASM compilation target `wasm32-unknown-unknown`.
* **Single Rust UI Layer:** All HUD elements, control panels, popups, and the 3D sky viewport are composed inside a single `agg-gui` widget tree. No HTML/CSS layer is used for application chrome — the browser only hosts the GL canvas that `agg-gui` draws into.

---

## 2. User Interface Design

Based on the application wireframe, the interface is optimized for single-handed mobile navigation and splits the viewport into a dedicated 3D canvas and a flat configuration tray. **All UI elements — including the 3D sky view itself — are composed as `agg-gui` widgets inside a single root `FlexColumn`,** ensuring consistent layout, theming, hit-testing, and keyboard/focus behavior across desktop and mobile WASM builds.

### Layout Specification
* **Upper Viewport (Main Canvas) — `SkyView` Custom Widget:** A full-bleed `agg-gui` widget implementing the `GlPaint` trait (the same hook used by agg-gui's 3D-cube demo) to issue native `wgpu` draw calls for the sky sphere into its assigned widget bounds.
  * Displays stars, planets, the Sun, and the Moon mapped relative to local orientation.
  * **Heads-Up Display (HUD) Horizon Tape:** A dynamic horizontal orientation strip rendered immediately above the control panel using `agg-gui`'s standard 2D `DrawCtx` (labels + separators), driven by the device's magnetometer/compass for the cardinal direction markers (`N`, `|`, `NE`, `|`, …).
* **Lower Configuration Control Panel — `agg-gui` `Container` + `FlexRow`:** Fixed bottom tray composed entirely of stock `agg-gui` widgets:
  * **Geolocation Button (Target Icon):** An `agg-gui` `Button` rendered with a Font Awesome crosshair glyph; its click callback invokes `navigator.geolocation` to fill position data instantly.
  * **Location Selection Controls:** `agg-gui` `ComboBox` widgets for `Country`, `City`, and `State`, with `TextField` instances providing search-as-you-type entry that drives the SQLite FTS5/Soundex queries described in §3.1.
  * **Toggles:** An `agg-gui` `ToggleSwitch` (or `Checkbox`) bound via shared `Rc<Cell<bool>>` state to instantly overlay or hide **Constellation Lines/Boundaries** in the `SkyView` widget.
* **Theming:** Dark, light, and system themes are inherited directly from `agg-gui`'s `Visuals` system so the HUD and control tray automatically follow the user's OS preference.

---

## 3. Component & Data Engineering Specifications

### 3.1 Geolocation Lookup System
To minimize dynamic network payloads, geographical data is split up into per-country files hosted statically on GitHub Pages.

* **Data Source Alternative:** Derived from the `countries-states-cities-database`.
* **Asset Payload Pipeline:** 1. The user picks or defaults to a country.
  2. The application requests `https://<user>.github.io/instant-astronomer/geo/[country_code].csv.gz`.
  3. The stream is passed to the browser's native `DecompressionStream("gzip")` via `wasm-bindgen` or handled via the `flate2` crate in Rust.
* **Database Pipeline (SQLite WASM):** * The application compiles a standalone SQLite WASM binary containing `-DSQLITE_ENABLE_FTS5` and `-DSQLITE_ENABLE_SOUNDEX`.
  * On asset decompression, an in-memory database (`:memory:`) table is constructed:
    ```sql
    CREATE TABLE cities (
        id INTEGER PRIMARY KEY,
        city TEXT,
        state TEXT,
        lat REAL,
        lng REAL,
        soundex_key TEXT
    );
    CREATE INDEX idx_cities_soundex ON cities(soundex_key);
    
    CREATE VIRTUAL TABLE cities_fts USING fts5(
        city, state, content='cities', content_rowid='id'
    );
    ```
  * **Search Ingestion Fallback Matrix:**
    * *Search-As-You-Type:* Queries are routed to `cities_fts` using prefix filters (`MATCH 'Denve*'`).
    * *Typo / Phonetic Lookup:* If FTS5 yields zero results, the query executes spelling-insensitive phonetic evaluation:
      ```sql
      SELECT city, state, lat, lng FROM cities WHERE soundex_key = soundex(?1);
      ```

### 3.2 Ephemeris & Astronomy Pipeline
Instead of tracking celestial bodies via tabular datasets, coordinates are determined algorithmically in real-time.

* **Fixed Stars Backdrop:** Managed using the **Yale Bright Star Catalog (BSC5)** stripped down to 5 vital data primitives: `ID`, `Right Ascension (RA)`, `Declination (Dec)`, `Visual Magnitude (V)`, and `Color Index (B-V)`. Total compressed asset footprint: **~150 KB**.
* **Dynamic Solar System Bodies:** Evaluated using **NASA JPL Keplerian Elements for Approximate Positions** relative to the standard J2000.0 Epoch.
  * **Mathematical Sequence for Planets:**
    1. Compute elapsed Julian centuries: $T = \frac{\text{Julian Date} - 2451545.0}{36525}$.
    2. Compute Heliocentric 3D position vectors $(X, Y, Z)$ for Earth and target planet using their 6 Keplerian orbital elements ($a, e, I, L, \barpi, \Omega$).
    3. Determine Geocentric position: $\vec{V}_{\text{geocentric}} = \vec{V}_{\text{planet}} - \vec{V}_{\text{earth}}$.
    4. Convert 3D Cartesian coordinates to Equatorial coordinates ($\text{Right Ascension}$, $\text{Declination}$).
  * **Lunar Tracking System:** Employs Jean Meeus's *Astronomical Algorithms* truncated literal theory using the principal 20 periodic terms for latitude and longitude to achieve sub-degree accuracy within a minimal computational envelope.

### 3.3 Constellation Infrastructure
* **Asset Payload:** Uses minified JSON/GeoJSON derived from `celestial_data` containing pre-computed `MULTILINESTRING` coordinate pairings for the 88 official IAU constellations. Total compressed asset footprint: **~25 KB**.
* **Coordinate Mapping:** Vector nodes are imported as static, immutable angular positions ($RA, Dec$) mapping cleanly into a 3D unit sphere space ($r = 1.0$).

---

## 4. Render & Coordinate Transformation Engine (`wgpu` via `agg-gui`)

The application maps the fixed celestial sphere primitives to local coordinates, which are transformed via the phone's hardware telemetry matrices. The render pipeline lives inside the `SkyView` widget's `GlPaint` callback, so it shares the same GL surface, device-scale, and event/clip context as every other `agg-gui` widget in the tree — no separate canvas, no DOM compositing.

```
[Equatorial Space: RA, Dec]
            │
            ▼ (Input: Latitude, Longitude, Local Sidereal Time)
[Horizontal Space: Altitude, Azimuth]
            │
            ▼ (Input: Device Orientation Matrix: Alpha, Beta, Gamma)
[Camera Viewspace Transformation]
            │
            ▼ (Input: Perspective Matrix)
[Screen Projection Viewport (wgpu Pipeline)]
```

### 4.1 Frame Mathematical Execution
For every rendering frame, the Rust engine evaluates the following pipeline:

1. **Local Sidereal Time ($LST$):** Derived from the current UTC timestamp and the selected city's longitude.
2. **Horizontal Transformation ($RA/Dec \rightarrow Alt/Az$):**
   $$\sin(Alt) = \sin(Dec)\sin(Lat) + \cos(Dec)\cos(Lat)\cos(LST - RA)$$
   $$\cos(Az) = \frac{\sin(Dec) - \sin(Alt)\sin(Lat)}{\cos(Alt)\cos(Lat)}$$
3. **Cartesian Projection:** Convert local $Alt/Az$ into local 3D coordinates on a unit sphere.
4. **Device Telemetry Integration:**
   * Listen to the browser's `deviceorientation` event, capturing Euler angles: `alpha` (yaw/compass), `beta` (pitch), and `gamma` (roll).
   * Transform these angles into a $3 \times 3$ rotation matrix inside Rust using `nalgebra`.
   * **Telemetry Smoothing Interface (Jitter Reduction):** Pass the raw rotation vector through a low-pass filter to eliminate magnetometer noise:
     $$\vec{\theta}_{\text{filtered}} = \vec{\theta}_{\text{filtered}} + \kappa \cdot (\vec{\theta}_{\text{raw}} - \vec{\theta}_{\text{filtered}})$$
     *(Set default smoothing modifier $\kappa = 0.12$ for responsive yet stabilized visual output).*

### 4.2 `wgpu` Render Pipeline Mechanics
* **Vertex Buffer Strategy:**
  * **Stars:** Kept as a single static vertex point cloud buffer. The vertex shader maps point size directly to visual magnitude $V$.
  * **Constellations:** Loaded into a static index buffer rendering as an optimized line list (`PrimitiveTopic::LineList`).
* **Uniform Shader Pipeline:** The calculated local time adjustments, geographic coordinate projections, and filtered device orientation matrices are packed tightly into a single unified uniform buffer passed to the GPU shader instance on every frame refresh. This minimizes CPU-to-GPU data transfer overhead.
* **Integration with `agg-gui`:** Inside `GlPaint::paint`, the `SkyView` widget receives its current bounds, viewport scissor, and device-pixel scale from `agg-gui`. It draws the celestial sphere first, then yields the GL context back to `agg-gui`, which composites the HUD horizon tape, control tray, and any overlays (tooltips, popups, theme toggle) on top using AGG-tessellated paths in the same frame.

---

## 5. Development Phase Roadmap

### Phase 1: Local Rust & Mathematical Framework
* Setup a standard Rust workspace with binary and module divisions; add `agg-gui` as the sole UI dependency.
* Implement Meeus and NASA JPL coordinate algorithms; validate outputs against known star charts.
* Build the `SkyView` widget as an `agg-gui` `GlPaint` implementor and project raw 3D coordinate arrays into its bounds via `wgpu` inside a native `agg-gui` host window.

### Phase 2: WebAssembly & Asset Packaging
* Configure compilation flags for the `wasm32-unknown-unknown` target ecosystem and reuse `agg-gui`'s WASM adapters (web keyboard/cursor/clipboard helpers) so the same widget tree runs in the browser unchanged.
* Construct the data pipelines: write data parsing scripts that trim the Yale Bright Star Catalog and country city structures, compress them into Gzip formats, and configure an asset folder.
* Integrate SQLite WASM compilation layers to ensure smooth compilation inside a sandboxed browser workspace.

### Phase 3: Telemetry Integration & Final Polish
* Wire up browser event listeners (`deviceorientation`, `navigator.geolocation`) into the Rust WASM loop using `web-sys`, feeding values into shared `Rc<Cell<_>>` state cells consumed by the `agg-gui` widget tree.
* Implement the low-pass stabilizing filters for device sensors.
* Compose the final control panel from stock `agg-gui` widgets (`Container`, `FlexRow`, `Button`, `ComboBox`, `TextField`, `ToggleSwitch`), wire reactive input states to re-trigger in-memory database queries, and host on GitHub Pages.