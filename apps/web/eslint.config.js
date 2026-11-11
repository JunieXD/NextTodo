import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import { defineConfig, globalIgnores } from "eslint/config";
import globals from "globals";
import tseslint from "typescript-eslint";

/** Methods Chromium 99 lacks that type checking lets through: those ES2023 adds to arrays, and the
 * DOM's (docs/web.md#浏览器). */
const missing = [
  ...["toSorted", "toReversed", "toSpliced", "with", "showPopover", "hidePopover", "togglePopover", "checkVisibility"].map((property) => ({ property })),
  { object: "AbortSignal", property: "timeout" },
  { object: "AbortSignal", property: "any" },
  { object: "URL", property: "canParse" },
  { object: "URL", property: "parse" },
];

export default defineConfig([
  globalIgnores(["dist", "dev-dist", "src/api/types"]),
  {
    files: ["**/*.{ts,tsx,js}"],
    extends: [js.configs.recommended, tseslint.configs.recommended, reactHooks.configs.flat["recommended-latest"]],
    languageOptions: { globals: { ...globals.browser, ...globals.node } },
    rules: { "@typescript-eslint/no-unused-vars": ["error", { ignoreRestSiblings: true }] },
  },
  {
    // What runs in the browser; the tests run in Node.
    files: ["src/**/*.{ts,tsx}"],
    ignores: ["src/**/*.test.ts", "src/test-setup.ts"],
    rules: { "no-restricted-properties": ["error", ...missing.map((m) => ({ ...m, message: "Chromium 99 lacks it (docs/web.md#浏览器)." }))] },
  },
]);
