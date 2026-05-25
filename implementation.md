# Instant-Astronomer.com — Implementation Specification

This document details the architectural design and implementation roadmap for **Instant-Astronomer.com**, a lightweight, serverless, client-side progressive web application. The app allows users to input their location (via manual entry or geolocation) and point their device at the night sky to view an interactive, real-time 3D overlay of stars, planets, the Sun, the Moon, and constellations.

The entire user interface — controls, panels, overlays, and the embedded 3D sky viewport — is built on **[`agg-gui`](https://github.com/larsbrubaker/agg-gui)**, an immediate-mode Rust GUI library with a unified widget set, theming system, and platform adapters that run identically on native desktop and in the browser via WASM.

---

## 1. System Architecture Overview

The system is designed to run entirely client-side inside a WebAssembly (WASM) runtime compiled from Rust, leveraging the device's native sensors and hardware-accelerated graphics via `agg-gui` (which renders through `wgpu`).

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
                  │                      │   agg-gui    │  │
                  │                      │ (widgets +   │  │
                  │                      │  wgpu draw)  │  │
                  │                      └──────────────┘  │
                  └────────────────────────────────────────┘
```

### Core Architecture Principles
* **Zero Backend Costs:** All data assets are hosted statically on GitHub Pages as compressed payloads.
* **Low Memory Footprint:** Data is parsed streaming or queried via an in-memory SQLite instances to prevent DOM memory exhaustion.
* **Sub-Millisecond Engine Interfacing:** All projection, coordinate transformations, and matrix transformations are performed in native Rust via WASM compilation target `wasm32-unknown-unknown`.
* **Unified Render Pipeline:** `agg-gui` owns the single `wgpu` surface and draws everything — control panel widgets, HUD horizon tape, *and* the sky sphere — through its own `DrawCtx` API. There is no separate 3D layer or parallel GL context; stars, constellation lines, and planet glyphs are issued as ordinary `agg-gui` draw calls from within the `SkyView` widget.

---

## 2. User Interface Design

Based on the application wireframe, the interface is optimized for single-handed mobile navigation and splits the viewport into a dedicated 3D canvas and a flat configuration tray. **All UI elements — including the 3D sky view itself — are composed as `agg-gui` widgets inside a single root `FlexColumn`,** ensuring consistent layout, theming, hit-testing, and keyboard/focus behavior across desktop and mobile WASM builds.

### Layout Specification
* **Upper Viewport (Main Canvas) — `SkyView` Custom Widget:** A full-bleed `agg-gui` widget that performs the celestial coordinate math in Rust each frame and emits the projected stars, planets, Sun, and Moon as ordinary `agg-gui` `DrawCtx` calls — filled circles for point sources (sized by visual magnitude), and line paths for constellation figures. No separate GL pipeline is required; `agg-gui`'s own `wgpu` backend rasterizes the sky alongside every other widget.
  * Displays stars, planets, the Sun, and the Moon mapped relative to local orientation.
  * **Heads-Up Display (HUD) Horizon Tape:** A dynamic horizontal orientation strip rendered immediately above the control panel using `agg-gui`'s standard `DrawCtx` (labels + separators), driven by the device's magnetometer/compass for the cardinal direction markers (`N`, `|`, `NE`, `|`, …).
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

## 4. Render & Coordinate Transformation Engine

The application maps the fixed celestial sphere primitives to local coordinates, which are transformed via the phone's hardware telemetry matrices. All rendering happens inside the `SkyView` widget's `draw` method, which receives the same `DrawCtx` (and underlying `wgpu` surface) that `agg-gui` uses for the control tray and HUD — there is no separate canvas, no separate GL context, and no DOM compositing.

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
[2D Screen Coordinates → agg-gui DrawCtx → wgpu]
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

### 4.2 `SkyView` Draw Mechanics
Each frame, `SkyView::draw(ctx: &mut DrawCtx, bounds: Rect)` walks its precomputed geometry and issues the following calls into `agg-gui`'s shared `wgpu` surface:

* **Stars:** A single pass over the projected star list emits one `ctx.fill_circle(p, radius_for_magnitude(V), color_for_bv(B-V))` per visible star. Stars whose computed altitude is below the horizon are skipped before any drawing call is made.
* **Constellations:** Each constellation figure is drawn as a single `ctx.stroke_path(...)` over its line segments, gated by the constellation-lines toggle.
* **Sun / Moon / Planets:** Rendered as larger filled circles (or small textured quads via `ctx.draw_image` for the Moon phase), tinted by body-specific colors.
* **HUD overlay:** After the sky pass returns, `agg-gui` continues drawing the horizon tape, control tray, and any popups/tooltips into the same `DrawCtx` — guaranteeing correct Z-order and a single GPU submit per frame.
* **Frame Cost:** Because every primitive flows through `agg-gui`'s existing batched `wgpu` path, no custom shaders, uniform buffers, or pipeline objects need to be authored for instant-astronomer.

---

## 5. Development Phase Roadmap

### Phase 1: Local Rust & Mathematical Framework
* Setup a standard Rust workspace with binary and module divisions; add `agg-gui` as the sole UI dependency (its `wgpu` backend doubles as the render layer).
* Implement Meeus and NASA JPL coordinate algorithms; validate outputs against known star charts.
* Build the `SkyView` widget as a standard `agg-gui` widget whose `draw` method projects star/planet coordinates and emits `DrawCtx` circles + line paths into a native `agg-gui` host window.

### Phase 2: WebAssembly & Asset Packaging
* Configure compilation flags for the `wasm32-unknown-unknown` target ecosystem and reuse `agg-gui`'s WASM adapters (web keyboard/cursor/clipboard helpers) so the same widget tree runs in the browser unchanged.
* Construct the data pipelines: write data parsing scripts that trim the Yale Bright Star Catalog and country city structures, compress them into Gzip formats, and configure an asset folder.
* Integrate SQLite WASM compilation layers to ensure smooth compilation inside a sandboxed browser workspace.

### Phase 3: Telemetry Integration & Final Polish
* Wire up browser event listeners (`deviceorientation`, `navigator.geolocation`) into the Rust WASM loop using `web-sys`, feeding values into shared `Rc<Cell<_>>` state cells consumed by the `agg-gui` widget tree.
* Implement the low-pass stabilizing filters for device sensors.
* Compose the final control panel from stock `agg-gui` widgets (`Container`, `FlexRow`, `Button`, `ComboBox`, `TextField`, `ToggleSwitch`), wire reactive input states to re-trigger in-memory database queries, and host on GitHub Pages.

---

## 6. Implementation Status (as of 2026-05-25)

This section tracks what's actually shipped vs. what's still on the
spec's wish list. Update inline whenever a chunk lands or a decision is
revised.

### Shipped
* **3-crate workspace** (`-core`, `-native`, `-wasm`) with shared widget
  tree built by `build_astronomer_app`. Core is `wasm32`-clean; shells
  inject capabilities through the `AstronomerPlatform` trait.
* **Sky math** — Julian date, GMST/LST, equatorial→horizontal, Y-up
  Cartesian projection, low-pass IMU smoothing (`κ = 0.12`). All in
  `instant-astronomer-core/src/math.rs`.
* **View rotation** — stored as a single `UnitQuaternion<f64>` so we're
  free of Euler-angle gimbal lock at the zenith. Mouse drags decompose
  as **world-axis yaw + camera-local pitch** (`q_world_yaw * view_quat
  * q_local_pitch`) — that's what keeps the horizon level under
  arbitrary diagonal drags. Pinned by
  `math::tests::horizon_stays_level_under_diagonal_drags`.
* **Solar System bodies** — Sun (low-precision Meeus), Moon (Meeus
  truncated theory, principal terms), Mercury/Venus/Mars/Jupiter/Saturn
  via simplified Keplerian elements relative to Earth's heliocentric
  position. Sub-degree accuracy budget per §3.2 (verified by
  `stars::tests::sun_position_at_j2000`).
* **Star catalog** — curated set of ~186 named bright stars to
  V ≈ 4.4, packed as a bundled CSV (`assets/bright_stars.csv`) parsed
  once at startup into a `OnceLock<Vec<Star>>`. Seeded 26-star const
  list (IDs 1..=26) backs the constellation-line index; extended catalog
  uses IDs 100+. Still well short of the BSC5 full ~9 k stars planned
  in §3.2.
* **Constellation lines** — Orion + Ursa Major asterisms wired through
  `CONSTELLATION_LINES` and toggled by a checkbox. **88 IAU
  constellation GeoJSON payload from §3.3 is NOT yet integrated.**
* **City search** — `instant_astronomer_core::cities` ships a ~150-city
  static index with Soundex phonetic fallback. The SQLite-WASM-with-FTS5
  pipeline described in §3.1 was **deferred** as overkill for the
  current city list size; we'll revisit if the catalog grows past
  ~10 k entries.
* **Geolocation** — `navigator.geolocation.getCurrentPosition` in WASM;
  a stub setting Greenwich coords on native.
* **Device orientation** — WASM listens for `deviceorientation` /
  `deviceorientationabsolute` and pushes into the core via
  `on_device_orientation(alpha, beta, gamma)`.
* **DST-correct local clock** — top-bar readout shows `UTC HH:MM ·
  local HH:MM` where `local` uses the platform-reported UTC offset
  (`time::OffsetDateTime::now_local()` on native, `Date.getTimezoneOffset()`
  on WASM). The offset is queried every paint so a DST transition while
  the app is open updates the clock automatically.
* **HUD** — horizon strip (compass tape), altitude ladder, centre
  reticle, alt = 0 great-circle line projected across the sky for
  "how far above the horizon am I looking?" feedback.
* **Calibrate-to-here button** — snapshots current compass heading into
  `calibration_yaw`, applied as a world-axis offset every frame.
* **GitHub Pages deploy workflow** — WASM bundle published on every
  push to `master`.

### Deferred / not yet implemented
* **Full Yale BSC5 (~9 k stars)** — current 186-star CSV is the
  intermediate. The compressed-asset-via-GitHub-Pages pipeline from
  §3.2 is still the long-term target.
* **88 IAU constellations** — only Orion + Ursa Major are drawn today.
* **Per-country city-DB gzipped payloads served from GitHub Pages**
  (§3.1) — current city list is statically bundled in the binary; works
  fine at our current scale.
* **SQLite-WASM in-memory DB + FTS5/Soundex** (§3.1) — replaced by an
  in-process Rust search with a Soundex fallback; revisit when catalog
  size warrants.
* **Country / State / City `ComboBox`** (§2 Layout Spec) — current UI
  uses a single text-field search.

### Known limitations
* The local time we display is the **device's** wall clock, not the
  wall clock at the searched lat/lng. Looking up "Tokyo" from a Denver
  device still shows Denver local time. Adding a lat/lng → IANA tz
  lookup would change that, at the cost of a tzdata-sized asset payload.
* `request_device` on iOS Safari often needs `requestPermission()` for
  `DeviceOrientationEvent` — the JS shim handles it where possible;
  a "Tap to enable orientation" affordance is on the wish list.