# Claude guidance for the Instant-Astronomer repo

## Architecture invariants

- 3-crate workspace: `instant-astronomer-core` (math, city DB, widgets,
  shared app builder), `instant-astronomer-native` (winit + wgpu present
  surface), `instant-astronomer-wasm` (cdylib + web-sys + WebGL2 present
  surface). Mirrors the Marbles / Solitaire shell pattern.
- **Single application, two host adapters.** `instant-astronomer-core`
  is the entire visible app. Native and WASM are platform shells that
  create a window/canvas, present the agg-gui paint, and forward events.
- `instant-astronomer-core` MUST stay `wasm32`-clean. No `tokio`, no
  `winit`, no `wgpu` calls. Capabilities are injected via traits
  (`AstronomerPlatform::request_geolocation` today; more to come).
- **All UI through agg-gui.** Sky-sphere stars, constellation lines, Sun /
  Moon / planet markers, the rolling horizon tape, and the configuration
  tray are all agg-gui widgets painting through `DrawCtx`. **There is no
  separate canvas/WebGL/wgpu 3-D pipeline.** If a needed primitive is
  missing, add it to `../agg-gui/agg-gui/src/…` first.
- **No HTML/CSS UI in the WASM shell.** The browser side may own a single
  `<canvas id="astronomer-canvas">`, request `navigator.geolocation`,
  subscribe to `deviceorientation` events, and forward results into Rust
  via `wasm-bindgen` exports. It must not draw buttons / labels / toggles
  / status text — those all live in `instant-astronomer-core`.
- **Typography / icons.** Render through agg-gui text widgets. Font
  Awesome icons (when added) load through agg-gui's icon-font path. If
  text scaling is insufficient on HiDPI / mobile, fix it in agg-gui, not
  here.
- **No accounts, no persistence backend.** All assets ship statically:
  star catalog, constellation lines, per-country city DB (per
  `implementation.md` section 3). If a feature requires a stateful
  backend it's out of scope.

## Coordinate / unit conventions

- Latitude / longitude are stored in **degrees** in the core state cells
  (matches the city DB, the user-facing readout, and the
  `navigator.geolocation` payload). The math layer in
  `instant-astronomer-core/src/math.rs` consumes radians — conversion
  happens at that boundary (`SkyViewWidget::paint` is the canonical
  spot).
- Yaw / pitch / roll are stored in **radians**. The WASM shell converts
  the browser's `deviceorientation` event (degrees) to radians at the
  boundary; the native shell does the same when piping mock or future
  sensor data in.
- RA / Dec in the star catalog are in radians at J2000.0 (pre-converted
  from the catalog's hours / degrees so the projection path never has to
  re-unit).
- `timestamp_ms` is Unix epoch milliseconds, UTC. The shell pumps this
  every frame so projections animate.
- Mouse events are y-up by the time they reach widgets (agg-gui flips
  once at the platform boundary). Don't re-flip.

## Local development uses agg-gui as a path dep — improve it as you go

The workspace `Cargo.toml` redirects `agg-gui` to `../agg-gui/agg-gui`
via `[patch.crates-io]`. This is the default state — every commit
assumes contributors run with the path override active.

When Instant-Astronomer needs an agg-gui feature that doesn't exist yet,
**add it to agg-gui** (not a one-off here). Workflow:

1. Make the change in `../agg-gui/agg-gui/src/…`.
2. Run instant-astronomer against the patched local crate
   (`cargo check --workspace`).
3. When stable, publish a new agg-gui version (Lars handles this).
4. CI builds against the published crates.io version. CI clones
   `larsbrubaker/agg-gui` as a sibling so the patch resolves there too.

Standalone clone, if you don't already have it as a sibling:

```powershell
git clone https://github.com/larsbrubaker/agg-gui.git ../agg-gui
```

## Build & test

```powershell
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

# Hot-reload native shell
cargo dev

# WASM
wasm-pack build instant-astronomer-wasm --target web --out-dir ../demo/public/pkg --no-typescript
```

`default-members` excludes `instant-astronomer-wasm` so plain
`cargo build` doesn't drag wasm-only deps into a native build.

## File-size guardrail

`instant-astronomer-core/tests/file_line_count.rs` enforces 800 lines per
first-party file. When it panics, **do a real refactor** — extract a
coherent sibling module. Don't compress code to slip under the limit.

## Test-first bug fixing

When a bug is reported:

1. Write a reproducing test first (it should fail).
2. Fix the bug with the minimal change to address the root cause.
3. Confirm the new test passes.

Never commit a bug fix without a test that would have caught it.

## Shell

This repo is developed on Windows / PowerShell. Cargo aliases and
`cargo-watch` invocations assume PowerShell; adapt as needed on
macOS / Linux.
