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

/** Ways of swallowing a failure: doing nothing with it, or only putting it in the console. A failure
 * goes on up to where it is handled, or to the one place that tells the user (docs/web.md#出错). */
const logging = "CallExpression[callee.object.name='console']";
/** A statement that does nothing with the failure: one that only logs, or returns nothing. */
const idle = `:matches(ExpressionStatement[expression.callee.object.name='console'], ReturnStatement:not([argument]), ReturnStatement[argument.callee.object.name='console'], ReturnStatement[argument.type='Identifier'][argument.name='undefined'])`;
/** What a promise calls on failing: `catch`'s callback, and `then`'s second. */
const onFailure = ["CallExpression[callee.property.name='catch'] > .arguments:first-child", "CallExpression[callee.property.name='then'] > .arguments:nth-child(2)"];
const swallowing = [
  `CatchClause > BlockStatement:not(:has(> :not(${idle})))`,
  ...onFailure.flatMap((f) => [
    `${f}:function > BlockStatement.body:not(:has(> :not(${idle})))`,
    `${f}ArrowFunctionExpression > :matches(${logging}, Identifier[name='undefined']).body`,
    `${f}MemberExpression[object.name='console']`,
  ]),
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
  {
    // A promise is awaited, handled, or `void`: what fails then goes to the one place that tells the
    // user (docs/web.md#出错).
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: { parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname } },
    rules: {
      "@typescript-eslint/no-floating-promises": "error",
      "no-restricted-syntax": ["error", ...swallowing.map((selector) => ({ selector, message: "Handle the failure, or let it go on up (docs/web.md#出错)." }))],
    },
  },
]);
