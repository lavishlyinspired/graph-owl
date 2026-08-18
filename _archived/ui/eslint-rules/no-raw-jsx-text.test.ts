import { RuleTester } from "eslint";
import { describe, it } from "vitest";
import plugin from "./no-raw-jsx-text.mjs";

// RuleTester calls through `describe`/`it` if present as globals; wiring
// them explicitly is what makes this run under Vitest rather than Mocha.
RuleTester.describe = describe;
RuleTester.it = it;

const ruleTester = new RuleTester({
  languageOptions: {
    ecmaVersion: 2022,
    sourceType: "module",
    parserOptions: { ecmaFeatures: { jsx: true } },
  },
});

ruleTester.run("no-raw-jsx-text", plugin.rules["no-raw-jsx-text"], {
  valid: [
    "const x = <Text>{label}</Text>;",
    "const x = <Text>{`${a} ${b}`}</Text>;",
    "const x = <Text> </Text>;",
    "const x = <Text>{3}</Text>;",
    "const x = <Text code>{triple(fact)}</Text>;",
    'const x = <code>{"raw source"}</code>;',
  ],
  invalid: [
    {
      code: "const x = <Text>hello</Text>;",
      errors: [{ messageId: "rawText" }],
    },
    {
      code: 'const x = <Text>{"hello"}</Text>;',
      errors: [{ messageId: "rawText" }],
    },
    {
      // Deeply nested, to prove the check isn't only looking at direct
      // children of the JSX root — the case a shallow implementation
      // would pass while still leaking a hard-coded string.
      code: "const x = <Card><Space><Text>hello</Text></Space></Card>;",
      errors: [{ messageId: "rawText" }],
    },
  ],
});
