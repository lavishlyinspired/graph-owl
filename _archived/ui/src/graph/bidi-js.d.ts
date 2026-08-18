/** `bidi-js` (MIT) ships no type declarations and no `@types/bidi-js`
 *  package exists — this covers only the surface `bidiLabel.ts` actually
 *  calls, not the library's full API. */
declare module "bidi-js" {
  interface EmbeddingLevelsResult {
    readonly levels: Uint8Array;
    readonly paragraphs: readonly { start: number; end: number; level: number }[];
  }

  interface Bidi {
    getEmbeddingLevels(text: string, explicitDirection?: "ltr" | "rtl"): EmbeddingLevelsResult;
    getReorderSegments(
      text: string,
      embeddingLevels: EmbeddingLevelsResult,
      start?: number,
      end?: number,
    ): readonly (readonly [number, number])[];
    getMirroredCharactersMap(
      text: string,
      levels: Uint8Array,
      start?: number,
      end?: number,
    ): Map<number, string>;
  }

  export default function bidiFactory(): Bidi;
}
