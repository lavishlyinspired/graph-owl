import { strings } from "../lib/strings";

/** Plan 122a A1: no tenancy exists yet (verified — every "workspace" hit in
 *  `crates/` is a *cargo* workspace, not a graph/pack isolation boundary).
 *  This renders the single implicit workspace, honestly, rather than
 *  faking a switchable list — see `plans/122a-graphowl-app.md` §1.3. */
export function WorkspaceSwitcher({ name }: { readonly name: string }) {
  return (
    <div
      title={strings.workspaceSingleNote}
      className="flex items-center gap-1.5 rounded-md border border-gowl-line px-2 py-1 text-[12px] text-gowl-t3"
    >
      <span>{name}</span>
    </div>
  );
}
