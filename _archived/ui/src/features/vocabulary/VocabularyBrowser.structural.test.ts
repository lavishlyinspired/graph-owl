import { describe, expect, it } from "vitest";
import source from "./VocabularyBrowser.tsx?raw";

/** Epic 42 Slice B's own named RED test: "a structural test asserts
 *  `VocabularyBrowser.tsx` has no vocabulary-specific branch." Reads the
 *  component's raw source, the same way the Rust side greps for a
 *  forbidden cross-crate reference — because a unit test exercising only
 *  the four real vocabularies cannot tell a genuinely parameterized
 *  component from one with four hardcoded paths that happen to agree with
 *  its tests. This is the file that closes that gap.
 *
 *  `?raw` (Vite's own import suffix, typed by `vite/client`) rather than
 *  `node:fs`: this project's `tsconfig.json` scopes `types` to
 *  `["vite/client"]` alone, so no other file in `src/` pulls in Node's
 *  ambient types either. */

// Every string a real VocabularyConfig.key currently uses
// (glossaryVocabulary/classificationVocabulary/domainVocabulary/
// ontologyPackVocabulary in vocabularies.ts). None may appear as a literal
// in the component — every one of them must arrive only through `config`.
const VOCABULARY_KEYS = ["glossary", "classification", "domain", "ontology-pack"];

describe("VocabularyBrowser.tsx has no vocabulary-specific branch", () => {
  it.each(VOCABULARY_KEYS)("never names the %s vocabulary as a string literal", (key) => {
    expect(source).not.toMatch(new RegExp(`["'\`]${key}["'\`]`, "i"));
  });

  it("never branches on a vocabulary kind via switch", () => {
    expect(source).not.toMatch(/\bswitch\s*\(/);
  });

  it("never imports the API client directly — only through config", () => {
    // Every vocabulary-specific type a hardcoded branch would need
    // (GlossaryTerm, Classification, Tag, Domain, OntologyPack, SkosRelation)
    // lives in ../../api and can only reach this file through that path —
    // closing this one import closes all of them at once.
    expect(source).not.toMatch(/from\s+["']\.\.\/\.\.\/api["']/);
  });
});
