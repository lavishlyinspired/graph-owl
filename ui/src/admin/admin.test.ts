/** Epic 41 Slice F's structural criterion: `SchemaForm` is the **only** form
 *  renderer in admin.
 *
 *  This asserts a property of the source rather than of a rendered component, and
 *  that is deliberate. The failure it guards is not a wrong pixel — it is somebody
 *  adding a second, hand-written connector form six months from now because it was
 *  quicker than extending the schema-driven one. No behavioural test catches that:
 *  both forms would work, and the second would quietly drift from the first until
 *  a field went missing from one of them.
 *
 *  A hundred connectors with hand-written forms is a hundred places for a field to
 *  go missing, and the one that goes missing is always the optional-looking one
 *  somebody actually needed.
 */

import { describe, expect, it } from "vitest";
// Vite's `?raw` rather than `node:fs`: `vite/client` already declares it, so this
// needs no `@types/node` and the tsconfig's deliberately narrow `types` list stays
// narrow. It also removes any dependence on the file's path on disk.
import APP from "../App.tsx?raw";

/** The admin region of `App.tsx`: from the admin page down to the app shell.
 *
 *  Scoped rather than whole-file, because the rest of the console legitimately
 *  contains forms — the description editor, the SPARQL box, the connector *run*
 *  form. The claim is about admin, not about the console. */
function adminSource(): string {
  const start = APP.indexOf("function AdminPage(");
  const end = APP.indexOf("export default function App()");
  expect(start, "AdminPage should exist in App.tsx").toBeGreaterThan(-1);
  expect(end, "the app shell should follow the admin section").toBeGreaterThan(start);
  return APP.slice(start, end);
}

describe("SchemaForm is the only form renderer in admin", () => {
  // The positive half: admin renders connector configuration *through* the pure
  // module. If this fails, either the panel was removed or it stopped using the
  // schema — both of which make the rest of this file vacuous.
  it("renders connector configuration from the schema module", () => {
    const admin = adminSource();

    expect(admin).toContain("schemaFields(");
    expect(admin).toContain("renderable(");
  });

  // **The guard.** Any second field-generating loop in admin is a second renderer.
  // `schemaFields(...).map` is the one permitted mapping over form fields; another
  // `.map` producing `<Input` from a locally-declared field list is what this
  // forbids.
  it("declares no field list of its own", () => {
    const admin = adminSource();

    // A hand-rolled form declares its fields as data — an array of names, labels
    // or field objects. The schema-driven one gets them from the connector.
    const localFieldLists = [
      /const\s+\w*[Ff]ields\s*[:=]\s*\[/,
      /const\s+\w*FORM\w*\s*=\s*\[/,
      /const\s+\w*[Ff]ormFields\w*\s*=/,
    ];

    for (const pattern of localFieldLists) {
      expect(
        pattern.test(admin),
        `admin declares its own field list (${pattern}) — configuration forms must come from the connector's JSON Schema`,
      ).toBe(false);
    }
  });

  // A secret must never be populated, in the component as well as in the pure
  // module. `schemaForm` refuses to supply one; this asserts the component does not
  // reintroduce it by reading a stored value into the input.
  it("never populates a secret field from a value", () => {
    const admin = adminSource();

    // The password input's `value` may fall back to `initial`, which `schemaForm`
    // guarantees is absent for secrets — but it must not read a secret from
    // anywhere else.
    expect(admin).not.toMatch(/secret[^\n]*value=\{[^}]*stored/i);
    expect(admin).toContain('field.kind === "secret" ? "password" : "text"');
  });

  // The unsupported case has to reach the screen. A field this build cannot render
  // must be visible as unsupported rather than drawn as a text box that submits the
  // wrong type — which is the whole reason `kindOf` has an `unsupported` verdict.
  it("shows an unrenderable field as unrenderable", () => {
    const admin = adminSource();

    expect(admin).toContain('field.kind === "unsupported"');
    expect(admin).toContain("not renderable");
  });
});

describe("the admin section shows what it cannot place", () => {
  // Orphans and cycles are separated by `hierarchy()` precisely so a screen can
  // say so. A panel that only rendered `roots` would silently drop teams, and a
  // team missing from an admin screen is one nobody can fix.
  it("renders orphaned and cyclic teams rather than only the tree", () => {
    const admin = adminSource();

    expect(admin).toContain("tree.orphans");
    expect(admin).toContain("tree.cyclic");
  });

  // The server's `409` carries counts by kind, which is the actionable part. A
  // generic "could not delete" would discard it.
  it("surfaces the delete refusal verbatim rather than a generic message", () => {
    const admin = adminSource();

    expect(admin).toContain("could not delete");
    expect(admin).toMatch(/e instanceof Error \? e\.message/);
  });
});
