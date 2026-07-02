// Browser bootstrap for Instant-Astronomer's single Rust/agg-gui app.
//
// The Rust wasm module (demo-wgpu's `web_shell`) owns everything
// platform-generic the moment it loads: canvas sizing, the frame loop,
// pointer / wheel / keyboard / clipboard input, DPR tracking, and client
// platform detection. This file owns only what genuinely cannot live in
// wasm:
//
// - loading the wasm module itself,
// - `navigator.geolocation` auto-request at page load (the in-app
//   "Locate me" button goes through Rust directly),
// - the `deviceorientation` bridge, because iOS requires a user-gesture
//   permission prompt (a DOM overlay) before events flow.
//
// Must NOT render visible UI beyond the canvas + the iOS sensor-permission
// gate — every button, label, toggle, status readout, etc. is painted by
// agg-gui inside the canvas. See CLAUDE.md.

// wasm-pack --no-typescript does not emit .d.ts files; we reference the
// generated module structurally instead.
type WasmModule = {
  default: (url?: string | URL | { module_or_path: string | URL }) => Promise<unknown>;
  on_device_orientation: (alpha: number, beta: number, gamma: number) => void;
  set_location_degrees: (latitudeDeg: number, longitudeDeg: number) => void;
};

// iOS DeviceOrientationEvent requires `requestPermission()` — typed as an
// add-on because the standard lib.dom.d.ts does not include it.
type DeviceOrientationConstructorIos = {
  requestPermission?: () => Promise<"granted" | "denied">;
};

type DeviceOrientationEventIos = DeviceOrientationEvent & {
  webkitCompassHeading?: number;
};

const canvas = document.getElementById("astronomer-canvas") as HTMLCanvasElement;
const permissionGate = document.getElementById("permission-gate") as HTMLDivElement;
const enableSensorsButton = document.getElementById("enable-sensors") as HTMLButtonElement;

function showBootError(err: unknown): void {
  console.error("instant-astronomer: failed to boot wasm app", err);
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return;
  }
  canvas.width = Math.max(1, canvas.clientWidth || window.innerWidth);
  canvas.height = Math.max(1, canvas.clientHeight || window.innerHeight);
  ctx.fillStyle = "#0a0a19";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.fillStyle = "#f2f2f7";
  ctx.font = "20px sans-serif";
  ctx.fillText("Instant-Astronomer failed to load.", 24, 48);
  ctx.font = "14px sans-serif";
  ctx.fillText(
    "Check that wasm-pack output exists at demo/public/pkg.",
    24,
    78,
  );
  ctx.fillText(
    String((err as Error)?.message ?? err ?? ""),
    24,
    102,
  );
}

async function loadWasm(): Promise<WasmModule> {
  // Resolve against `document.baseURI` so the URL is correct regardless
  // of where Vite places the bundle (under `/assets/` after build).
  // `import.meta.env.BASE_URL` was returning `"/"` on GitHub Pages
  // under the `/instant-astronomer/` sub-path which broke the load.
  const url = new URL("pkg/instant_astronomer_wasm.js", document.baseURI).href;
  const mod = (await import(/* @vite-ignore */ url)) as WasmModule;
  const wasmUrl = new URL("pkg/instant_astronomer_wasm_bg.wasm", document.baseURI).href;
  // Module init runs the Rust `#[wasm_bindgen(start)]`, which boots the
  // whole shell (input, frame loop, rendering).
  await mod.default({ module_or_path: wasmUrl });
  return mod;
}

/// Subscribe to the browser's orientation events.
///
/// Strategy:
///  - Prefer `deviceorientationabsolute` (Android Chrome) — `alpha` there is
///    referenced to true north and is the value we want.
///  - On iOS Safari, fall back to plain `deviceorientation` plus
///    `webkitCompassHeading` (clockwise from north). Convert to the
///    counter-clockwise yaw the rotation matrix expects: `360 - heading`.
function subscribeOrientation(wasm: WasmModule): void {
  let usingAbsolute = false;

  const handleAbsolute = (ev: DeviceOrientationEvent) => {
    if (ev.alpha === null || ev.beta === null || ev.gamma === null) {
      return;
    }
    usingAbsolute = true;
    wasm.on_device_orientation(ev.alpha, ev.beta, ev.gamma);
  };

  const handleRelative = (ev: DeviceOrientationEvent) => {
    // If we are already getting absolute events, ignore the relative ones.
    if (usingAbsolute) {
      return;
    }
    const iosEv = ev as DeviceOrientationEventIos;
    let alpha = ev.alpha ?? 0;
    if (typeof iosEv.webkitCompassHeading === "number") {
      alpha = 360 - iosEv.webkitCompassHeading;
    }
    wasm.on_device_orientation(alpha, ev.beta ?? 0, ev.gamma ?? 0);
  };

  window.addEventListener(
    "deviceorientationabsolute",
    handleAbsolute as EventListener,
    { passive: true },
  );
  window.addEventListener("deviceorientation", handleRelative, { passive: true });
}

async function maybeRequestOrientationPermission(): Promise<boolean> {
  const ctor = (
    window as unknown as { DeviceOrientationEvent?: DeviceOrientationConstructorIos }
  ).DeviceOrientationEvent;
  if (!ctor || typeof ctor.requestPermission !== "function") {
    // Non-iOS browsers — `deviceorientation` is available without a prompt.
    return true;
  }
  try {
    const result = await ctor.requestPermission();
    return result === "granted";
  } catch (err) {
    console.warn("instant-astronomer: orientation permission request failed", err);
    return false;
  }
}

function needsIosPermissionGate(): boolean {
  const ctor = (
    window as unknown as { DeviceOrientationEvent?: DeviceOrientationConstructorIos }
  ).DeviceOrientationEvent;
  return !!ctor && typeof ctor.requestPermission === "function";
}

/// Auto-request geolocation on load. The agg-gui Geolocation button still
/// works as a manual retry if the user denies the first prompt or wants to
/// refresh.
function requestGeolocation(wasm: WasmModule): void {
  if (!navigator.geolocation) {
    return;
  }
  navigator.geolocation.getCurrentPosition(
    (pos) => {
      wasm.set_location_degrees(pos.coords.latitude, pos.coords.longitude);
      console.log(
        `instant-astronomer: geolocation lat=${pos.coords.latitude.toFixed(3)} ` +
          `lng=${pos.coords.longitude.toFixed(3)}`,
      );
    },
    (err) => {
      console.warn(
        `instant-astronomer: geolocation denied (code ${err.code}): ${err.message}`,
      );
    },
    { enableHighAccuracy: false, maximumAge: 60_000, timeout: 10_000 },
  );
}

async function boot(): Promise<void> {
  const wasm = await loadWasm();
  requestGeolocation(wasm);

  if (needsIosPermissionGate()) {
    permissionGate.dataset.visible = "true";
    enableSensorsButton.addEventListener(
      "click",
      async () => {
        const granted = await maybeRequestOrientationPermission();
        if (granted) {
          subscribeOrientation(wasm);
        }
        permissionGate.dataset.visible = "false";
      },
      { once: true },
    );
  } else {
    subscribeOrientation(wasm);
  }
}

void boot().catch(showBootError);
