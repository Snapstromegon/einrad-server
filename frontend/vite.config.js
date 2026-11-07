import { fileURLToPath, URL } from "node:url";
import { createKubb } from "@kubb/core";
import { applyDefaults } from "unplugin-kubb";
import { defineConfig } from "vite-plus";
import kubbConfig from "./kubb.config.js";
import consts from "./rollup-lib/consts.js";
import getGitStatus from "./rollup-lib/getGitStatus.js";

const openapiPath = fileURLToPath(new URL("../openapi.json", import.meta.url));
/** The backend for the dev server's proxy. */
const backend = "http://localhost:3000";

/**
 * Generates the API types and fetch functions (src/ts/gen) from openapi.json
 * with Kubb once at the start of each build (also `vp test` and `vp dev`). In
 * watch mode and on the dev server they are regenerated when openapi.json
 * changes. Unchanged files aren't rewritten, so watch mode doesn't loop.
 */
const apiClient = () => {
  const generate = async () => {
    const result = await createKubb(applyDefaults(kubbConfig)).generate();
    if (!result.success) {
      const error = result.diagnostics.find((d) => d.severity === "error");
      throw new Error(`Kubb: ${error?.message ?? "generation failed"}`);
    }
  };
  return {
    name: "api-client",
    async buildStart() {
      this.addWatchFile(openapiPath);
      await generate();
    },
    configureServer(server) {
      server.watcher.add(openapiPath);
      server.watcher.on("change", (file) => {
        if (file === openapiPath) {
          generate().catch((e) => server.config.logger.error(e.message));
        }
      });
    },
  };
};

export default defineConfig(({ mode }) => ({
  fmt: {
    printWidth: 80,
    // Leaves the HTML and CSS inside Lit templates as written.
    embeddedLanguageFormatting: "off",
    ignorePatterns: ["src/ts/gen/**"],
  },
  lint: {
    plugins: ["oxc", "typescript", "unicorn"],
    jsPlugins: [
      "eslint-plugin-wc",
      "eslint-plugin-lit",
      {
        name: "vite-plus",
        specifier: "vite-plus/oxlint-plugin",
      },
    ],
    categories: {
      correctness: "warn",
    },
    env: {
      builtin: true,
    },
    ignorePatterns: ["src/ts/gen/**"],
    rules: {
      "constructor-super": "error",
      "for-direction": "error",
      "getter-return": "error",
      "no-async-promise-executor": "error",
      "no-case-declarations": "error",
      "no-class-assign": "error",
      "no-compare-neg-zero": "error",
      "no-cond-assign": "error",
      "no-const-assign": "error",
      "no-constant-binary-expression": "error",
      "no-constant-condition": "error",
      "no-control-regex": "error",
      "no-debugger": "error",
      "no-delete-var": "error",
      "no-dupe-class-members": "error",
      "no-dupe-else-if": "error",
      "no-dupe-keys": "error",
      "no-duplicate-case": "error",
      "no-empty": "error",
      "no-empty-character-class": "error",
      "no-empty-pattern": "error",
      "no-empty-static-block": "error",
      "no-ex-assign": "error",
      "no-extra-boolean-cast": "error",
      "no-fallthrough": "error",
      "no-func-assign": "error",
      "no-global-assign": "error",
      "no-import-assign": "error",
      "no-invalid-regexp": "error",
      "no-irregular-whitespace": "error",
      "no-loss-of-precision": "error",
      "no-misleading-character-class": "error",
      "no-new-native-nonconstructor": "error",
      "no-nonoctal-decimal-escape": "error",
      "no-obj-calls": "error",
      "no-prototype-builtins": "error",
      "no-redeclare": "error",
      "no-regex-spaces": "error",
      "no-self-assign": "error",
      "no-setter-return": "error",
      "no-shadow-restricted-names": "error",
      "no-sparse-arrays": "error",
      "no-this-before-super": "error",
      "no-unassigned-vars": "error",
      "no-undef": "error",
      "no-unexpected-multiline": "error",
      "no-unreachable": "error",
      "no-unsafe-finally": "error",
      "no-unsafe-negation": "error",
      "no-unsafe-optional-chaining": "error",
      "no-unused-labels": "error",
      "no-unused-private-class-members": "error",
      "no-unused-vars": "error",
      "no-useless-assignment": "error",
      "no-useless-backreference": "error",
      "no-useless-catch": "error",
      "no-useless-escape": "error",
      "no-with": "error",
      "preserve-caught-error": "error",
      "require-yield": "error",
      "use-isnan": "error",
      "valid-typeof": "error",
      "no-empty-function": "error",
      "wc/no-constructor-attributes": "error",
      "wc/no-invalid-element-name": "error",
      "wc/no-self-class": "error",
      "wc/attach-shadow-constructor": "error",
      "wc/guard-super-call": "error",
      "wc/no-child-traversal-in-attributechangedcallback": "error",
      "wc/no-child-traversal-in-connectedcallback": "error",
      "wc/no-closed-shadow-root": "error",
      "wc/no-constructor-params": "error",
      "wc/no-customized-built-in-elements": "error",
      "wc/no-invalid-extends": "error",
      "wc/no-typos": "error",
      "wc/require-listener-teardown": "error",
      "lit/attribute-names": "error",
      "lit/attribute-value-entities": "error",
      "lit/ban-attributes": "error",
      "lit/binding-positions": "error",
      "lit/lifecycle-super": "error",
      "lit/no-classfield-shadowing": "error",
      "lit/no-duplicate-template-bindings": "error",
      "lit/no-invalid-escape-sequences": "error",
      "lit/no-invalid-html": "error",
      "lit/no-legacy-imports": "error",
      "lit/no-legacy-template-syntax": "error",
      "lit/no-native-attributes": "error",
      "lit/no-private-properties": "error",
      "lit/no-property-change-update": "error",
      "lit/no-template-arrow": "error",
      "lit/no-template-bind": "error",
      "lit/no-template-map": "error",
      "lit/no-this-assign-in-render": "error",
      "lit/no-useless-template-literals": "error",
      "lit/no-value-attribute": "error",
      "lit/prefer-nothing": "error",
      "lit/prefer-query-decorators": "error",
      "lit/prefer-static-styles": "error",
      "lit/quoted-expressions": "error",
      "lit/value-after-constraints": "error",
      "typescript/adjacent-overload-signatures": "error",
      "typescript/array-type": "error",
      "typescript/ban-tslint-comment": "error",
      "typescript/class-literal-property-style": "error",
      "typescript/consistent-generic-constructors": "error",
      "typescript/consistent-indexed-object-style": "error",
      "typescript/consistent-type-assertions": "error",
      "typescript/consistent-type-definitions": "error",
      "typescript/no-confusing-non-null-assertion": "error",
      "typescript/no-inferrable-types": "error",
      "typescript/prefer-for-of": "error",
      "typescript/prefer-function-type": "error",
      "vite-plus/prefer-vite-plus-imports": "error",
      // Lit binds event listeners in templates to the element.
      "typescript/unbound-method": "off",
      "typescript/no-floating-promises": "off",
    },
    overrides: [
      {
        files: ["test/**/*.js"],
        env: { node: true, browser: true },
      },
      {
        files: ["**/*.ts", "**/*.tsx", "**/*.mts", "**/*.cts"],
        rules: {
          "constructor-super": "off",
          "getter-return": "off",
          "no-class-assign": "off",
          "no-const-assign": "off",
          "no-dupe-class-members": "off",
          "no-dupe-keys": "off",
          "no-func-assign": "off",
          "no-import-assign": "off",
          "no-new-native-nonconstructor": "off",
          "no-obj-calls": "off",
          "no-redeclare": "off",
          "no-setter-return": "off",
          "no-this-before-super": "off",
          "no-undef": "off",
          "no-unreachable": "off",
          "no-unsafe-negation": "off",
          "no-var": "error",
          "no-with": "off",
          "prefer-const": "error",
          "prefer-rest-params": "error",
          "prefer-spread": "error",
        },
      },
      {
        files: ["src/**/*.ts"],
        rules: {
          "no-unused-vars": "off",
        },
      },
    ],
    options: {
      typeAware: true,
      typeCheck: true,
    },
  },
  root: "src",
  test: {
    // The tests live next to src, not in it.
    root: fileURLToPath(new URL(".", import.meta.url)),
    include: ["test/**/*.test.js"],
  },
  // Copied unchanged, e.g. src/public/assets/… is served as /assets/….
  publicDir: "public",
  plugins: [consts({ git: getGitStatus }), apiClient()],
  build: {
    outDir: "../../static",
    emptyOutDir: true,
    target: "esnext",
    sourcemap: mode !== "release",
  },
  server: {
    proxy: {
      // Includes the websocket of remote playback (/api/ws/player).
      "/api": { target: backend, ws: true },
      "/songs": backend,
      "/openapi.json": backend,
      "/swagger": backend,
    },
  },
}));
