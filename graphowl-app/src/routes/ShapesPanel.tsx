import { useRef, useState } from "react";
import {
  importShapes,
  isAdminOnlyError,
  previewShapes,
  runValidation,
  seedCoreShapes,
  type RunValidationResult,
  type ShapeDetail,
  type ShapeFlake,
  type ShapesPreviewResult,
} from "../lib/api";
import { GST_SHAPE_TEMPLATES } from "../lib/gstShapeTemplates";
import { strings } from "../lib/strings";

/** Plan 126: the Validation screen used to show all zeros with no way to
 *  tell "never validated" from "genuinely clean", and no way to trigger
 *  anything except a raw `curl`. Slice 4 closes the follow-on gaps: seeding
 *  showed a raw flake count with no way to know which shapes those were or
 *  whether they found anything, and there was nothing to select and try
 *  against the GST pack short of hand-writing Turtle from scratch. */
export function ShapesPanel({
  computedAtT,
  onValidated,
}: {
  readonly computedAtT: number;
  readonly onValidated: () => void;
}) {
  const [seeding, setSeeding] = useState(false);
  const [seedResult, setSeedResult] = useState<ShapesPreviewResult | null>(null);
  const [seedError, setSeedError] = useState<string | null>(null);
  const [showSeedFlakes, setShowSeedFlakes] = useState(false);

  const [running, setRunning] = useState(false);
  const [runResult, setRunResult] = useState<RunValidationResult | null>(null);
  const [runError, setRunError] = useState<string | null>(null);

  const [document, setDocument] = useState("");
  const [template, setTemplate] = useState("");
  const [previewing, setPreviewing] = useState(false);
  const [committing, setCommitting] = useState(false);
  const [preview, setPreview] = useState<ShapesPreviewResult | null>(null);
  const [previewedDocument, setPreviewedDocument] = useState<string | null>(null);
  const [committed, setCommitted] = useState(false);
  const [authorError, setAuthorError] = useState<string | null>(null);
  const [showFlakes, setShowFlakes] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);

  const doSeed = async () => {
    setSeeding(true);
    setSeedError(null);
    try {
      const result = await seedCoreShapes();
      setSeedResult(result);
      setShowSeedFlakes(false);
      onValidated();
    } catch (e) {
      setSeedError(isAdminOnlyError(e) ? strings.shapesAdminOnly : strings.governError);
    } finally {
      setSeeding(false);
    }
  };

  const doRun = async () => {
    setRunning(true);
    setRunError(null);
    try {
      const result = await runValidation();
      setRunResult(result);
      onValidated();
    } catch {
      setRunError(strings.governError);
    } finally {
      setRunning(false);
    }
  };

  const resetAuthorResult = () => {
    setPreview(null);
    setPreviewedDocument(null);
    setCommitted(false);
    setShowFlakes(false);
  };

  const doPreview = async () => {
    if (document.trim().length === 0) return;
    setPreviewing(true);
    setAuthorError(null);
    setCommitted(false);
    try {
      const result = await previewShapes(document);
      setPreview(result);
      setPreviewedDocument(document);
      setShowFlakes(false);
    } catch {
      setAuthorError(strings.governError);
    } finally {
      setPreviewing(false);
    }
  };

  const doCommit = async () => {
    setCommitting(true);
    setAuthorError(null);
    try {
      const result = await importShapes(document);
      setPreview(result);
      setCommitted(true);
      onValidated();
    } catch (e) {
      setAuthorError(isAdminOnlyError(e) ? strings.shapesAdminOnly : strings.governError);
    } finally {
      setCommitting(false);
    }
  };

  const doClear = () => {
    setDocument("");
    setTemplate("");
    setAuthorError(null);
    resetAuthorResult();
    if (fileInput.current) fileInput.current.value = "";
  };

  const onUpload = (file: File | undefined) => {
    if (!file) return;
    file.text().then((text) => {
      setDocument(text);
      setTemplate("");
      resetAuthorResult();
    });
  };

  const onSelectTemplate = (name: string) => {
    setTemplate(name);
    const found = GST_SHAPE_TEMPLATES.find((t) => t.name === name);
    if (found) {
      setDocument(found.document);
      resetAuthorResult();
    }
  };

  const canCommit = preview?.kind === "checked" && previewedDocument === document && !committed;

  return (
    <div className="mb-6 rounded-lg border border-gowl-line bg-gowl-panel p-5">
      <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-[19px] font-semibold text-gowl-t1">{strings.shapesTitle}</h2>
          <p className="text-[15px] text-gowl-t5">{strings.shapesSubtitle}</p>
        </div>
        <div className="flex items-center gap-4">
          <span className="font-mono text-[13.5px] text-gowl-t5">
            {computedAtT > 0
              ? `${strings.shapesLastCheckedPrefix} ${computedAtT}`
              : strings.shapesLastCheckedNever}
          </span>
          <button
            type="button"
            disabled={seeding}
            onClick={() => void doSeed()}
            className="rounded-md border border-gowl-line-2 px-3 py-1.5 text-[13.5px] text-gowl-t2 disabled:opacity-40"
          >
            {seeding ? strings.shapesSeeding : strings.shapesSeed}
          </button>
          <button
            type="button"
            disabled={running}
            onClick={() => void doRun()}
            className="rounded-md bg-gowl-accent px-3 py-1.5 text-[13.5px] font-semibold text-gowl-accent-on disabled:opacity-40"
          >
            {running ? strings.shapesRunning : strings.shapesRun}
          </button>
        </div>
      </div>

      {seedError && <p className="mb-3 text-[14px] text-gowl-bad">{seedError}</p>}
      {seedResult && seedResult.kind === "checked" && (
        <ShapesResultView
          result={seedResult}
          heading={strings.shapesWhatWasSeeded}
          showFlakes={showSeedFlakes}
          onToggleFlakes={() => setShowSeedFlakes((v) => !v)}
        />
      )}
      {runError && <p className="mb-3 text-[14px] text-gowl-bad">{runError}</p>}
      {runResult && (
        <ShapesKpiRow
          shapes={runResult.shapes}
          conforms={runResult.conforms}
          violations={runResult.violations}
          warnings={runResult.warnings}
          info={runResult.info}
        />
      )}

      <div className="mt-5 border-t border-gowl-line pt-5">
        <h3 className="mb-1 text-[16px] font-semibold text-gowl-t1">{strings.shapesAuthorTitle}</h3>
        <p className="mb-3 text-[14px] text-gowl-t5">{strings.shapesAuthorSubtitle}</p>

        <div className="mb-3">
          <select
            value={template}
            onChange={(e) => onSelectTemplate(e.target.value)}
            aria-label={strings.shapesTemplates}
            className="w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 text-[13.5px] text-gowl-t1"
          >
            <option value="">{strings.shapesTemplatesPlaceholder}</option>
            {GST_SHAPE_TEMPLATES.map((t) => (
              <option key={t.name} value={t.name}>
                {t.name}
              </option>
            ))}
          </select>
          {template && (
            <p className="mt-1 text-[12.5px] text-gowl-t5">
              {GST_SHAPE_TEMPLATES.find((t) => t.name === template)?.description}
            </p>
          )}
        </div>

        <textarea
          value={document}
          onChange={(e) => {
            setDocument(e.target.value);
            setTemplate("");
            setCommitted(false);
          }}
          placeholder={strings.shapesPlaceholder}
          rows={8}
          className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input p-3 font-mono text-[13.5px] text-gowl-t1"
        />

        <div className="mb-3 flex flex-wrap items-center gap-2">
          <input
            ref={fileInput}
            type="file"
            accept=".ttl,text/turtle"
            onChange={(e) => onUpload(e.target.files?.[0])}
            className="text-[13px] text-gowl-t5"
            aria-label={strings.shapesUpload}
          />
          <button
            type="button"
            disabled={previewing || document.trim().length === 0}
            onClick={() => void doPreview()}
            className="rounded-md border border-gowl-line-2 px-3 py-1.5 text-[13.5px] text-gowl-t2 disabled:opacity-40"
          >
            {previewing ? strings.shapesPreviewing : strings.shapesPreview}
          </button>
          <button
            type="button"
            disabled={!canCommit || committing}
            onClick={() => void doCommit()}
            className="rounded-md bg-gowl-accent px-3 py-1.5 text-[13.5px] font-semibold text-gowl-accent-on disabled:opacity-40"
          >
            {committing ? strings.shapesCommitting : strings.shapesCommit}
          </button>
          <button
            type="button"
            onClick={doClear}
            className="rounded-md border border-gowl-line-2 px-3 py-1.5 text-[13.5px] text-gowl-t5"
          >
            {strings.shapesClear}
          </button>
        </div>

        {authorError && <p className="mb-3 text-[14px] text-gowl-bad">{authorError}</p>}

        {!preview && !authorError && <p className="text-[14px] text-gowl-t5">{strings.shapesEmpty}</p>}

        {preview && preview.kind === "syntaxError" && (
          <div className="rounded-md border border-gowl-bad-border bg-gowl-bad-bg p-3">
            <p className="text-[14px] font-semibold text-gowl-bad">{strings.shapesSyntaxError}</p>
            <p className="mt-1 font-mono text-[13.5px] text-gowl-t2">
              {preview.line != null ? `line ${preview.line}${preview.column != null ? `, col ${preview.column}` : ""}: ` : ""}
              {preview.message}
            </p>
          </div>
        )}

        {preview && preview.kind === "checked" && (
          <ShapesResultView
            result={preview}
            heading={committed ? strings.shapesWhatWasImported : strings.shapesWhatWasPreviewed}
            footnote={committed ? strings.shapesCommittedBody : strings.shapesPreviewedNotCommitted}
            showFlakes={showFlakes}
            onToggleFlakes={() => setShowFlakes((v) => !v)}
          />
        )}
      </div>
    </div>
  );
}

/** Shared by the seed section and the author section — both now return the
 *  same `ShapesPreviewResult["kind" = "checked"]` shape, so both get the
 *  same readable rendering rather than the seed path showing only a raw
 *  flake count while preview/import show everything. */
function ShapesResultView({
  result,
  heading,
  footnote,
  showFlakes,
  onToggleFlakes,
}: {
  readonly result: Extract<ShapesPreviewResult, { readonly kind: "checked" }>;
  readonly heading: string;
  readonly footnote?: string;
  readonly showFlakes: boolean;
  readonly onToggleFlakes: () => void;
}) {
  return (
    <div className="mb-3">
      <ShapesKpiRow
        shapes={result.shapes}
        conforms={result.conforms}
        violations={result.violations}
        warnings={result.warnings}
        info={result.info}
      />

      {result.refusedShapes.length > 0 && (
        <div className="mt-3 rounded-md border border-gowl-amber-border bg-gowl-amber-bg p-3">
          <p className="mb-1 text-[13.5px] font-semibold text-gowl-amber">{strings.shapesRefusedShapes}</p>
          {result.refusedShapes.map((message, i) => (
            <p key={i} className="font-mono text-[13px] text-gowl-t2">
              {message}
            </p>
          ))}
        </div>
      )}

      {footnote && <p className="mt-3 text-[14px] text-gowl-t3">{footnote}</p>}

      <p className="mb-1 mt-3 text-[13.5px] font-semibold text-gowl-t2">{heading}</p>
      <ShapeDetailsList details={result.shapeDetails} />

      <button type="button" onClick={onToggleFlakes} className="mt-2 text-[13.5px] text-gowl-accent">
        {`${showFlakes ? strings.shapesHideFlakes : strings.shapesShowFlakes} (${result.flakes.length})`}
      </button>
      {showFlakes && <FlakeList flakes={result.flakes} />}

      {result.sample.length > 0 && (
        <div className="mt-3 overflow-hidden rounded-lg border border-gowl-line">
          <div className="grid grid-cols-4 gap-3 border-b border-gowl-line bg-gowl-panel-2 px-3 py-1.5 font-mono text-[12.5px] tracking-wider text-gowl-t6">
            <span>{strings.validationColShape}</span>
            <span>{strings.validationColFocusNode}</span>
            <span>{strings.validationColConstraint}</span>
            <span>{strings.validationColSeverity}</span>
          </div>
          {result.sample.map((v, i) => (
            <div
              key={i}
              className="grid grid-cols-4 items-center gap-3 border-b border-gowl-row px-3 py-2 text-[13.5px] last:border-b-0"
            >
              <span className="truncate font-mono text-gowl-t2">{v.shape}</span>
              <span className="truncate text-gowl-t1">{v.focusNode}</span>
              <span className="truncate font-mono text-gowl-t2">{v.constraint}</span>
              <span
                className={
                  v.severity === "violation"
                    ? "font-semibold text-gowl-bad"
                    : v.severity === "warning"
                      ? "font-semibold text-gowl-amber"
                      : "text-gowl-t5"
                }
              >
                {v.severity}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function targetLabel(target: ShapeDetail["target"]): string {
  if (typeof target.value === "string") return target.value;
  if (Array.isArray(target.value)) return target.value.join(", ");
  return target.kind;
}

function ShapeDetailsList({ details }: { readonly details: readonly ShapeDetail[] }) {
  if (details.length === 0) {
    return <p className="text-[13.5px] text-gowl-t5">{strings.shapesNoConstraints}</p>;
  }
  return (
    <div className="space-y-2">
      {details.map((shape) => (
        <div key={shape.id} className="rounded-md border border-gowl-line-2 bg-gowl-panel-2 p-3">
          <div className="mb-1 flex flex-wrap items-baseline gap-x-2">
            <span className="font-mono text-[13.5px] font-semibold text-gowl-t1">{shape.id}</span>
            <span className="text-[12.5px] text-gowl-t5">
              {strings.shapesTargets} <span className="font-mono text-gowl-t3">{targetLabel(shape.target)}</span>
            </span>
          </div>
          {shape.constraints.length === 0 ? (
            <p className="text-[13px] text-gowl-t5">{strings.shapesNoConstraints}</p>
          ) : (
            <ul className="ml-4 list-disc space-y-0.5 text-[13px] text-gowl-t3">
              {shape.constraints.map((c, i) => (
                <li key={i}>
                  {c.path && <span className="font-mono text-gowl-accent">{c.path}</span>} {c.detail}
                </li>
              ))}
            </ul>
          )}
        </div>
      ))}
    </div>
  );
}

function ShapesKpiRow({
  shapes,
  conforms,
  violations,
  warnings,
  info,
}: {
  readonly shapes: number;
  readonly conforms: boolean;
  readonly violations: number;
  readonly warnings: number;
  readonly info: number;
}) {
  return (
    <div className="mb-1 flex flex-wrap gap-6 font-mono text-[13.5px]">
      <span className="text-gowl-t5">
        {strings.shapesKpiShapes} <span className="text-gowl-t1">{shapes}</span>
      </span>
      <span className="text-gowl-t5">
        {strings.shapesKpiConforms}{" "}
        <span className={conforms ? "text-gowl-ok" : "text-gowl-bad"}>
          {conforms ? strings.shapesConformsYes : strings.shapesConformsNo}
        </span>
      </span>
      <span className="text-gowl-t5">
        {strings.shapesKpiViolations} <span className="text-gowl-bad">{violations}</span>
      </span>
      <span className="text-gowl-t5">
        {strings.shapesKpiWarnings} <span className="text-gowl-amber">{warnings}</span>
      </span>
      <span className="text-gowl-t5">
        {strings.shapesKpiInfo} <span className="text-gowl-t1">{info}</span>
      </span>
    </div>
  );
}

function FlakeList({ flakes }: { readonly flakes: readonly ShapeFlake[] }) {
  return (
    <div className="mt-2 max-h-[240px] overflow-y-auto rounded-md border border-gowl-line-2 bg-gowl-panel-2 p-2 font-mono text-[12.5px] text-gowl-t2">
      {flakes.map((f, i) => (
        <div key={i} className="border-b border-gowl-row py-1 last:border-b-0">
          <span className="text-gowl-accent">{f.s}</span> <span className="text-gowl-t5">{f.p}</span>{" "}
          <span>{f.o}</span>
        </div>
      ))}
    </div>
  );
}
