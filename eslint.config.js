import js from "@eslint/js";
import solid from "eslint-plugin-solid/configs/typescript";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: [
      "**/dist/**",
      "**/coverage/**",
      "apps/client/android/**",
      "apps/client/ios/**",
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.strict,
  {
    files: ["apps/**/*.{ts,tsx}"],
    ...solid,
    languageOptions: {
      ...solid.languageOptions,
      globals: globals.browser,
    },
  },
  {
    files: ["**/*.{js,mjs}"],
    languageOptions: { globals: globals.node },
  },
);
