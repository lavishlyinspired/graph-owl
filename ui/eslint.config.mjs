import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import globals from "globals";
import localRules from "./eslint-rules/no-raw-jsx-text.mjs";

export default tseslint.config(
  { ignores: ["dist", "src/generated", ".stryker-tmp"] },
  {
    files: ["**/*.{ts,tsx}"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
    },
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
      local: localRules,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_" }],
      "react-refresh/only-export-components": ["warn", { allowConstantExport: true }],
      // Epic 39 Slice E decision 7 — enforced from here forward. The
      // existing App.tsx has pre-Slice-F literals the rule was not yet
      // running against; those are `eslint-disable`d file-locally with a
      // dated note rather than silently exempted, so new code in the same
      // file is still caught.
      "local/no-raw-jsx-text": "error",
    },
  },
);
