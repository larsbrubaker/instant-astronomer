import { defineConfig } from "vite";
import basicSsl from "@vitejs/plugin-basic-ssl";
import { phoneShots } from "./vite-plugins/phone-shots";

// GitHub Pages serves the demo at
// https://larsbrubaker.github.io/instant-astronomer/
// so all asset paths must be prefixed accordingly. `./` works both there
// and locally under `vite dev`.
export default defineConfig(({ command }) => ({
  base: "./",
  // Stamped into the bundle and appended to the wasm-pack asset URLs in
  // main.ts. The pkg/ files are served with stable (unhashed) names, so
  // without this browsers keep serving a stale cached wasm long after a
  // deploy — phones "never got the update".
  define: {
    __BUILD_ID__: JSON.stringify(Date.now().toString(36)),
  },
  // `command === "serve"` covers `vite` (dev) and `vite preview`. The
  // basic-ssl plugin serves a self-signed cert so the dev server is a
  // secure context, which the phone needs for navigator.geolocation and
  // DeviceOrientation. `host: true` binds 0.0.0.0 so a phone on the same
  // Wi-Fi can reach it via the printed Network URL. `phoneShots` adds the
  // dev-only `/__shot` screenshot upload endpoint (see
  // vite-plugins/phone-shots.ts and the README phone-testing section).
  plugins: command === "serve" ? [basicSsl(), phoneShots()] : [],
  server: { host: true },
  preview: { host: true },
}));
