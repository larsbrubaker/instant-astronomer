import { defineConfig } from "vite";

// GitHub Pages serves the demo at
// https://larsbrubaker.github.io/instant-astronomer/
// so all asset paths must be prefixed accordingly. `./` works both there
// and locally under `vite dev`.
export default defineConfig({
  base: "./",
});
