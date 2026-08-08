/** The export dialog — Epic 42 / Phase 3 item 3.15.
 *
 *  Six formats put on the wire with no way to reach them but a raw URL —
 *  this is the console surface for `scope`/`asOf` filtering and the
 *  preview count that lets a reader see how big an export is before
 *  committing to a download.
 */
import { useState } from "react";
import { Alert, Button, Input, Modal, Select, Space, Typography } from "antd";
import { ApiError, api, downloadExport } from "../../api";
import { EXPORT_FORMATS, exportPath, previewPath, type ExportFilters } from "./exportFormats";

const { Text, Paragraph } = Typography;

const COPY = {
  title: "Export the graph",
  intro:
    "Only what you are authorized to see is ever included — narrowing by scope or a historical instant only ever removes rows, never adds any this principal could not otherwise reach.",
  formatLabel: "Format",
  scopeLabel: "Scope (optional)",
  scopePlaceholder: "An FQN prefix, e.g. hdfc-core.payments",
  asOfLabel: "As of (optional)",
  close: "Close",
  preview: "Preview",
  download: "Download",
  previewFailed: "the preview did not load",
  downloadFailed: "the export did not download",
};

function previewMessage(nodes: number, edges: number): string {
  const nodeWord = nodes === 1 ? "node" : "nodes";
  const edgeWord = edges === 1 ? "edge" : "edges";
  return `This export would contain ${nodes} ${nodeWord} and ${edges} ${edgeWord}.`;
}

export function ExportDialog({
  open,
  onClose,
  defaultAsOf,
}: {
  readonly open: boolean;
  readonly onClose: () => void;
  /** The header's own "view the catalog as it was" selection, if one is
   *  set — reused as this dialog's starting point rather than making a
   *  reader re-enter a timestamp they already picked once, but still
   *  overridable: an export's own historical view is not required to
   *  match whatever the rest of the console happens to be showing. */
  readonly defaultAsOf?: string | null;
}) {
  const [formatKey, setFormatKey] = useState(EXPORT_FORMATS[0].key);
  const [scope, setScope] = useState("");
  const [asOf, setAsOf] = useState(defaultAsOf ?? "");
  const [preview, setPreview] = useState<{ nodes: number; edges: number } | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);

  const format = EXPORT_FORMATS.find((f) => f.key === formatKey) ?? EXPORT_FORMATS[0];
  const filters: ExportFilters = {
    scope: scope.trim() || null,
    asOf: asOf.trim() || null,
  };

  const runPreview = () => {
    setPreviewing(true);
    setFailed(null);
    void api
      .exportPreview(previewPath(filters))
      .then((result) => setPreview(result))
      .catch((error: unknown) => {
        setFailed(error instanceof ApiError ? (error.problem.detail ?? error.problem.title) : COPY.previewFailed);
        setPreview(null);
      })
      .finally(() => setPreviewing(false));
  };

  const runDownload = () => {
    setDownloading(true);
    setFailed(null);
    void downloadExport(exportPath(format, filters), `graph-export-${format.key}`)
      .catch((error: unknown) => {
        setFailed(error instanceof ApiError ? (error.problem.detail ?? error.problem.title) : COPY.downloadFailed);
      })
      .finally(() => setDownloading(false));
  };

  return (
    <Modal
      open={open}
      title={COPY.title}
      onCancel={onClose}
      footer={[
        <Button key="close" onClick={onClose}>
          {COPY.close}
        </Button>,
        <Button key="preview" loading={previewing} onClick={runPreview}>
          {COPY.preview}
        </Button>,
        <Button key="download" type="primary" loading={downloading} onClick={runDownload}>
          {COPY.download}
        </Button>,
      ]}
    >
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <Paragraph type="secondary" style={{ margin: 0, fontSize: 13 }}>
          {COPY.intro}
        </Paragraph>

        <div>
          <Text strong style={{ fontSize: 12 }}>
            {COPY.formatLabel}
          </Text>
          <Select
            style={{ width: "100%", marginTop: 4 }}
            value={formatKey}
            onChange={(value: string) => setFormatKey(value)}
            options={EXPORT_FORMATS.map((f) => ({ value: f.key, label: f.label }))}
          />
        </div>

        <div>
          <Text strong style={{ fontSize: 12 }}>
            {COPY.scopeLabel}
          </Text>
          <Input
            style={{ marginTop: 4 }}
            placeholder={COPY.scopePlaceholder}
            value={scope}
            onChange={(e) => setScope(e.target.value)}
          />
        </div>

        <div>
          <Text strong style={{ fontSize: 12 }}>
            {COPY.asOfLabel}
          </Text>
          <Input
            type="datetime-local"
            style={{ marginTop: 4 }}
            value={asOf ? asOf.slice(0, 16) : ""}
            onChange={(e) => setAsOf(e.target.value ? new Date(e.target.value).toISOString() : "")}
          />
        </div>

        {failed && <Alert type="error" showIcon message={failed} />}

        {preview && <Alert type="info" showIcon message={previewMessage(preview.nodes, preview.edges)} />}
      </Space>
    </Modal>
  );
}
