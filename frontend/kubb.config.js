import { adapterOas } from "@kubb/adapter-oas";
import { pluginFetch } from "@kubb/plugin-fetch";
import { pluginTs } from "@kubb/plugin-ts";
import { defineConfig } from "kubb/config";

/**
 * Generates the API types and one fetch function per operation from the
 * backend's openapi.json into src/ts/gen (not committed). The Vite plugin in
 * vite.config.js runs it on each build and on the dev server.
 */
export default defineConfig({
  root: import.meta.dirname,
  input: "../openapi.json",
  output: { path: "./src/ts/gen", barrel: { type: "named" } },
  adapter: adapterOas({ integerType: "number" }),
  plugins: [
    pluginTs({ output: { path: "types.ts" }, enum: { type: "literal" } }),
    pluginFetch({ output: { path: "clients" } }),
  ],
});
