import { useEffect, useState } from "react";
import {
  dryRunOntologyEdit,
  isAdminOnlyError,
  previewOntologyEdit,
  runSparql,
  saveOntologyEdit,
  type OntologyDryRunResult,
  type OntologyPreviewResult,
  type OntologySaveResult,
} from "../../lib/api";
import { formatOntologyCheckSummary, formatOntologySaveSummary, ontologyEditorGraphQuery } from "../../lib/ontology/ontologyEditor";
import { ontologyModelFromSparqlRows, type OntologyModel } from "../../lib/ontology/ontologyModel";
import { strings } from "../../lib/strings";

const PANEL = "rounded-lg border border-gowl-line bg-gowl-panel p-4";

function RejectedList({ rejected }: { readonly rejected: readonly (readonly [string, string])[] }) {
  if (rejected.length === 0) return null;
  return (
    <div className="mt-2">
      <div className="font-mono text-[13.5px] tracking-widest text-gowl-t6">
        {strings.studioOntologyEditorRejectedHeading}
      </div>
      {rejected.map(([subject, reason]) => (
        <p key={subject} className="text-[16px] text-gowl-bad">
          {`${subject}: ${reason}`}
        </p>
      ))}
    </div>
  );
}

/** Ports the archived builder's Code tab onto the exact backend contract
 *  it already called (`/ontology-editor/{preview,dry-run,save}`, Epic 42
 *  Slice G) — see `plans/ontology-editor.md`. Not a visual/canvas editor:
 *  authoring is Turtle text, validated and saved through the same shapes
 *  and reasoning gate every RDF import already goes through. */
export function OntologyEditorPanel() {
  const [document, setDocument] = useState("");
  const [preview, setPreview] = useState<OntologyPreviewResult | null>(null);
  const [check, setCheck] = useState<OntologyDryRunResult | null>(null);
  const [save, setSave] = useState<OntologySaveResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [forbidden, setForbidden] = useState(false);
  const [current, setCurrent] = useState<OntologyModel | null>(null);

  const loadCurrent = () => {
    runSparql(ontologyEditorGraphQuery())
      .then((result) => setCurrent(ontologyModelFromSparqlRows(result.rows)))
      .catch(() => setCurrent(null));
  };

  useEffect(loadCurrent, []);

  const runPreview = () => {
    setBusy(true);
    setForbidden(false);
    previewOntologyEdit(document)
      .then(setPreview)
      .catch((error: unknown) => setForbidden(isAdminOnlyError(error)))
      .finally(() => setBusy(false));
  };

  const runCheck = () => {
    setBusy(true);
    setForbidden(false);
    dryRunOntologyEdit(document)
      .then(setCheck)
      .catch((error: unknown) => setForbidden(isAdminOnlyError(error)))
      .finally(() => setBusy(false));
  };

  const runSave = () => {
    setBusy(true);
    setForbidden(false);
    saveOntologyEdit(document)
      .then((result) => {
        setSave(result);
        if (result.kind === "saved") loadCurrent();
      })
      .catch((error: unknown) => setForbidden(isAdminOnlyError(error)))
      .finally(() => setBusy(false));
  };

  return (
    <div className="flex flex-col gap-4">
      <p className="text-[15px] text-gowl-t6">{strings.studioOntologyEditorScopeNote}</p>

      {forbidden && <p className="text-[16.5px] text-gowl-bad">{strings.studioOntologyEditorAdminOnly}</p>}

      {current && (
        <p className="font-mono text-[15px] text-gowl-t6">
          {`${strings.studioOntologyEditorCurrentlyDeclared}: ${current.classes.length} classes · ${current.relationships.length} relationships`}
        </p>
      )}

      <textarea
        value={document}
        onChange={(event) => setDocument(event.target.value)}
        placeholder={strings.studioOntologyEditorPlaceholder}
        spellCheck={false}
        className="min-h-[280px] rounded-md border border-gowl-line-2 bg-gowl-input p-3 font-mono text-[16px] text-gowl-t1"
      />

      <div className="flex gap-2">
        <button
          type="button"
          disabled={busy}
          onClick={runPreview}
          className="rounded-md border border-gowl-line-2 px-3 py-1.5 text-[16px] text-gowl-t2 disabled:opacity-40"
        >
          {strings.studioOntologyEditorPreview}
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={runCheck}
          className="rounded-md border border-gowl-line-2 px-3 py-1.5 text-[16px] text-gowl-t2 disabled:opacity-40"
        >
          {strings.studioOntologyEditorCheck}
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={runSave}
          className="rounded-md bg-gowl-accent px-3 py-1.5 text-[16px] text-gowl-bg disabled:opacity-40"
        >
          {strings.studioOntologyEditorSave}
        </button>
      </div>

      {preview && (
        <div className={PANEL}>
          {preview.kind === "syntaxError" ? (
            <p className="text-[16.5px] text-gowl-bad">{`Syntax error: ${preview.message}`}</p>
          ) : (
            <div>
              <div className="font-mono text-[13.5px] tracking-widest text-gowl-t6">
                {strings.studioOntologyEditorDeclaredHeading}
              </div>
              <p className="text-[16.5px] text-gowl-t2">{preview.declared.join(", ") || "—"}</p>
            </div>
          )}
        </div>
      )}

      {check && (
        <div className={PANEL}>
          <p className="text-[16.5px] text-gowl-t2">{formatOntologyCheckSummary(check)}</p>
          {check.kind === "checked" && <RejectedList rejected={check.rejected} />}
        </div>
      )}

      {save && (
        <div className={PANEL}>
          <p className={`text-[16.5px] ${save.kind === "saved" ? "text-gowl-ok" : "text-gowl-bad"}`}>
            {formatOntologySaveSummary(save)}
          </p>
          {save.kind === "saved" && <RejectedList rejected={save.rejected} />}
        </div>
      )}
    </div>
  );
}
