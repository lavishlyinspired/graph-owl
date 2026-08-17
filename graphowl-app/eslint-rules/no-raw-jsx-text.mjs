/** Epic 39 Slice E decision 7: "No user-visible literal in a component."
 *  Retrofitting i18n after the fact means touching every component; this
 *  rule is what makes the alternative — externalizing from the first
 *  commit — actually enforced rather than aspirational.
 *
 *  Flags non-whitespace `JSXText` and unparenthesized string-literal JSX
 *  expression children, e.g. `<Text>hello</Text>` or `<Text>{"hello"}</Text>`.
 *  Does not flag: whitespace-only text (JSX formatting), template literals
 *  with interpolation (usually a computed label, not a fixed string),
 *  numeric/boolean literals (not user-visible copy), and `<Text code>` /
 *  `<code>` children (rendered source, not prose — a triple like `s p o` is
 *  not a string this rule exists to catch). */

const CODE_ELEMENT_NAMES = new Set(["code", "Code"]);

function isCodeElement(node) {
  let current = node.parent;
  while (current) {
    if (current.type === "JSXElement") {
      const name = current.openingElement.name;
      if (name.type === "JSXIdentifier" && CODE_ELEMENT_NAMES.has(name.name)) return true;
      const attrs = current.openingElement.attributes ?? [];
      const codeProp = attrs.some(
        (a) => a.type === "JSXAttribute" && a.name?.name === "code" && a.value === null,
      );
      if (codeProp) return true;
    }
    current = current.parent;
  }
  return false;
}

const rule = {
  meta: {
    type: "problem",
    docs: {
      description: "disallow a user-visible string literal directly in JSX — externalize it",
    },
    schema: [],
    messages: {
      rawText: "User-visible text must be externalized, not written directly in JSX.",
    },
  },
  create(context) {
    return {
      JSXText(node) {
        if (node.value.trim() === "") return;
        if (isCodeElement(node)) return;
        context.report({ node, messageId: "rawText" });
      },
      "JSXExpressionContainer > Literal"(node) {
        if (typeof node.value !== "string" || node.value.trim() === "") return;
        if (isCodeElement(node)) return;
        context.report({ node, messageId: "rawText" });
      },
    };
  },
};

export default {
  rules: {
    "no-raw-jsx-text": rule,
  },
};
