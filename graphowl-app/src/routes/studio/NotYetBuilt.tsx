import { strings } from "../../lib/strings";

/** Honest placeholder for a Studio tab whose backend does not exist yet —
 *  same "state the real gap, don't fabricate" pattern as `contradictions.tsx`. */
export function NotYetBuilt({ body }: { readonly body: string }) {
  return (
    <div className="max-w-[560px] rounded-lg border border-gowl-line bg-gowl-panel p-5">
      <div className="mb-2 font-mono text-[9.5px] tracking-widest text-gowl-t6">{strings.studioNotYetBuiltTitle}</div>
      <p className="text-[12.5px] leading-relaxed text-gowl-t3">{body}</p>
    </div>
  );
}
