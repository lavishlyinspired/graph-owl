/** The console's "Agent" tab — a live chat over `agent_service`'s
 *  streaming API (`integrations/langchain/agent_service/`), embedded in
 *  the shipped console per an explicit product decision to reverse the
 *  earlier "keep it outside the shipped product" call. The agent runtime
 *  itself still runs as a separate Python/LangGraph process — this
 *  component is a *consumer* of that service over HTTP/SSE, the same
 *  relationship every other feature here has with graph-owl-server
 *  (`api.ts`), not the agent runtime being ported into the engine
 *  (`plans/00j-language-boundaries.md`'s refusal on that point still
 *  holds; see `agentClient.ts`'s own comment).
 *
 *  **Two or more questions run at once, genuinely — not a queue with a
 *  spinner.** Each `askQuestion` call gets its own `threadId` and its own
 *  `EventSource`; nothing here waits for one investigation to finish
 *  before starting the next. `agentStreamsRef` keeps every open
 *  connection alive regardless of which thread is currently selected in
 *  the sidebar, so switching away from a running thread never pauses it
 *  — proven first as a standalone page
 *  (`agent_service/static/index.html`) before this component existed;
 *  this is that same design, in React.
 *
 *  **Agent selector is real infrastructure, not a stub for one item.**
 *  `AGENTS` is a plain list a second entry drops into without touching
 *  anything else — the picker, the request payload, and the per-thread
 *  state all key off `agentId` already. */

import { useCallback, useEffect, useRef, useState } from "react";
import {
  Alert,
  Button,
  Empty,
  Input,
  Layout,
  List,
  Modal,
  Select,
  Space,
  Spin,
  Tag,
  Typography,
} from "antd";
import {
  CheckOutlined,
  CloseCircleFilled,
  FileTextOutlined,
  LoadingOutlined,
  PaperClipOutlined,
  RobotOutlined,
  SendOutlined,
  SwapOutlined,
} from "@ant-design/icons";
import {
  askQuestion,
  listProviders,
  readFile,
  streamAnswer,
  uploadFile,
  type FileContent,
  type ProviderOption,
  type ToolActivity,
  type UploadedFile,
} from "./agentClient";

const { Text, Title } = Typography;
const { Sider, Content } = Layout;

interface Agent {
  id: string;
  label: string;
}

/** The one agent that exists today. A second entry here is the entire
 *  change needed to offer a second agent in the picker — nothing else in
 *  this file names "reconciliation" specifically. */
const AGENTS: Agent[] = [{ id: "reconciliation", label: "Reconciliation Agent" }];

type ThreadStatus = "running" | "done" | "error";

interface ThreadState {
  agentId: string;
  question: string;
  status: ThreadStatus;
  text: string;
  activity: ToolActivity[];
  error: string | null;
  files: UploadedFile[];
}

const COPY = {
  title: "Agent",
  intro:
    "Ask the reconciliation agent a question. You can ask another one immediately — it runs alongside this one, not after it.",
  placeholder: "Ask the Reconciliation Agent…",
  emptyTranscript: "Ask a question below.",
  emptySidebar: "No questions yet.",
  submitError: "Could not reach the agent service.",
  attachTitle: "Attach a file (e.g. a GSTR-2B export or purchase register, as JSON)",
  fallbackNotice: "Switched to a fallback model to continue this investigation.",
  toolFailed: "✕",
  selectProvider: "Model provider",
  selectModel: "Model",
};

/** Rotates while a thread has nothing concrete to show yet — the same
 *  role Cursor/Claude/ChatGPT's own animated "Thinking…" labels play,
 *  rather than a single static word for however long the first model
 *  turn takes. */
const THINKING_LABELS = [
  "Thinking",
  "Cogitating",
  "Catapulating",
  "Percolating",
  "Ruminating",
  "Noodling",
  "Synthesizing",
  "Pondering",
  "Untangling",
  "Marinating",
];

const THINKING_LABEL_INTERVAL_MS = 1400;

export function AgentChat() {
  const [agentId, setAgentId] = useState<string>(AGENTS[0]?.id ?? "reconciliation");
  const [threads, setThreads] = useState<Record<string, ThreadState>>({});
  const [activeThreadId, setActiveThreadId] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [submitError, setSubmitError] = useState<string | null>(null);
  // Files uploaded but not yet attached to a sent question — cleared
  // once `handleAsk` sends them, mirroring how `input` clears on send.
  const [stagedFiles, setStagedFiles] = useState<UploadedFile[]>([]);
  const [uploadError, setUploadError] = useState<string | null>(null);
  const [previewFile, setPreviewFile] = useState<FileContent | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  // Every provider the agent service currently reports as configured and
  // reachable — fetched once on mount. An empty list (no provider
  // reachable, or the service itself unreachable) leaves the selector
  // empty and `handleAsk` sends no provider/model at all, which is
  // exactly the pre-picker default behaviour, unchanged.
  const [providers, setProviders] = useState<ProviderOption[]>([]);
  const [selectedProviderId, setSelectedProviderId] = useState<string | null>(null);
  const [selectedModelId, setSelectedModelId] = useState<string | null>(null);

  useEffect(() => {
    listProviders()
      .then((list) => {
        setProviders(list);
        const first = list[0];
        if (first) {
          setSelectedProviderId(first.id);
          setSelectedModelId(first.models[0]?.id ?? null);
        }
      })
      .catch(() => {
        // No providers reachable right now — selector stays empty.
      });
  }, []);

  const handleProviderChange = useCallback(
    (providerId: string) => {
      setSelectedProviderId(providerId);
      const provider = providers.find((p) => p.id === providerId);
      setSelectedModelId(provider?.models[0]?.id ?? null);
    },
    [providers],
  );

  // Every open EventSource, keyed by threadId — a ref rather than state
  // because opening/closing a connection is not itself something the UI
  // renders; only the ThreadState it feeds does that.
  const streamsRef = useRef<Map<string, () => void>>(new Map());

  useEffect(() => {
    const streams = streamsRef.current;
    return () => {
      for (const close of streams.values()) close();
    };
  }, []);

  const updateThread = useCallback((threadId: string, patch: Partial<ThreadState>) => {
    setThreads((prev) => {
      const current = prev[threadId];
      if (!current) return prev;
      return { ...prev, [threadId]: { ...current, ...patch } };
    });
  }, []);

  const handleFilesSelected = useCallback(async (fileList: FileList | null) => {
    if (!fileList || fileList.length === 0) return;
    setUploadError(null);
    for (const file of Array.from(fileList)) {
      try {
        const text = await file.text();
        const uploaded = await uploadFile(file.name, file.type || "application/json", text);
        setStagedFiles((prev) => [...prev, uploaded]);
      } catch (e) {
        setUploadError(e instanceof Error ? e.message : `Could not read ${file.name}.`);
      }
    }
  }, []);

  const openPreview = useCallback(async (file: UploadedFile) => {
    setPreviewFile({ ...file, content: "" });
    setPreviewError(null);
    setPreviewLoading(true);
    try {
      const full = await readFile(file.fileId);
      setPreviewFile(full);
    } catch (e) {
      setPreviewError(e instanceof Error ? e.message : "Could not load this file.");
    } finally {
      setPreviewLoading(false);
    }
  }, []);

  const handleAsk = useCallback(async () => {
    const question = input.trim();
    if (!question) return;
    setInput("");
    setSubmitError(null);
    const filesForThisQuestion = stagedFiles;
    setStagedFiles([]);

    let threadId: string;
    try {
      ({ threadId } = await askQuestion(
        question,
        filesForThisQuestion.map((f) => f.fileId),
        selectedProviderId ?? undefined,
        selectedModelId ?? undefined,
      ));
    } catch (e) {
      setSubmitError(e instanceof Error ? e.message : COPY.submitError);
      setStagedFiles(filesForThisQuestion); // put them back — the question never went out
      return;
    }

    setThreads((prev) => ({
      ...prev,
      [threadId]: {
        agentId,
        question,
        status: "running",
        text: "",
        activity: [],
        error: null,
        files: filesForThisQuestion,
      },
    }));
    setActiveThreadId(threadId);

    // Each "message" event is a *delta* — the agent service sends only
    // the newly-produced piece, not the whole answer so far (see
    // agent_service/streaming.py's StreamChunk docstring) — so this
    // reads the latest React state via the functional form of
    // setThreads rather than closing over a stale `text` from when
    // handleAsk started. Appending, not replacing, is what makes the
    // answer grow token by token the way Cursor/Claude/ChatGPT's own
    // chat views do, instead of the text flickering through
    // fragments and settling on whatever chunk happened to arrive last.
    const close = streamAnswer(threadId, (event) => {
      if (event.kind === "message") {
        setThreads((prev) => {
          const current = prev[threadId];
          if (!current) return prev;
          return { ...prev, [threadId]: { ...current, text: current.text + event.text } };
        });
      } else if (event.kind === "update") {
        setThreads((prev) => {
          const current = prev[threadId];
          if (!current) return prev;
          return {
            ...prev,
            [threadId]: { ...current, activity: [...current.activity, event.data] },
          };
        });
      } else {
        updateThread(threadId, { status: event.status, error: event.error });
        streamsRef.current.delete(threadId);
      }
    });
    streamsRef.current.set(threadId, close);
  }, [input, agentId, updateThread, stagedFiles, selectedProviderId, selectedModelId]);

  const threadEntries = Object.entries(threads).sort(([a], [b]) => a.localeCompare(b));
  const active = activeThreadId ? threads[activeThreadId] : undefined;

  return (
    <Layout style={{ background: "transparent", height: "100%" }}>
      <Sider width={280} style={{ background: "transparent", paddingRight: 16 }}>
        <Title level={3} style={{ margin: 0, fontWeight: 600, fontSize: 16 }}>
          {COPY.title}
        </Title>
        <Text type="secondary">{COPY.intro}</Text>
        <div style={{ marginTop: 16, marginBottom: 8 }}>
          <Select<string>
            value={agentId}
            onChange={setAgentId}
            style={{ width: "100%" }}
            options={AGENTS.map((a) => ({ value: a.id, label: a.label }))}
          />
        </div>
        <div style={{ marginBottom: 8 }}>
          <Select<string>
            value={selectedProviderId ?? undefined}
            onChange={handleProviderChange}
            style={{ width: "100%" }}
            placeholder={COPY.selectProvider}
            aria-label={COPY.selectProvider}
            disabled={providers.length === 0}
            options={providers.map((p) => ({ value: p.id, label: p.label }))}
          />
        </div>
        <div style={{ marginBottom: 16 }}>
          <Select<string>
            value={selectedModelId ?? undefined}
            onChange={setSelectedModelId}
            style={{ width: "100%" }}
            placeholder={COPY.selectModel}
            aria-label={COPY.selectModel}
            disabled={!selectedProviderId}
            options={(providers.find((p) => p.id === selectedProviderId)?.models ?? []).map(
              (m) => ({ value: m.id, label: m.label }),
            )}
          />
        </div>
        {threadEntries.length === 0 ? (
          <Empty description={COPY.emptySidebar} image={Empty.PRESENTED_IMAGE_SIMPLE} />
        ) : (
          <List
            size="small"
            dataSource={threadEntries}
            renderItem={([id, thread]) => (
              <List.Item
                key={id}
                onClick={() => setActiveThreadId(id)}
                style={{
                  cursor: "pointer",
                  background: id === activeThreadId ? "rgba(74,144,226,0.12)" : undefined,
                  padding: "6px 8px",
                  border: "none",
                }}
              >
                <Space size={6} align="start">
                  <StatusDot status={thread.status} />
                  <Text ellipsis style={{ maxWidth: 220 }}>
                    {thread.question}
                  </Text>
                </Space>
              </List.Item>
            )}
          />
        )}
      </Sider>
      <Content style={{ display: "flex", flexDirection: "column", height: "100%" }}>
        <div style={{ flex: 1, overflowY: "auto", padding: "0 8px", whiteSpace: "pre-wrap" }}>
          {!active ? (
            <Text type="secondary">{COPY.emptyTranscript}</Text>
          ) : (
            <>
              <div
                style={{
                  marginBottom: 12,
                  padding: "8px 12px",
                  borderRadius: 8,
                  background: "rgba(127,127,127,0.08)",
                  fontWeight: 500,
                  whiteSpace: "pre-wrap",
                }}
              >
                {active.question}
              </div>
              {active.files.length > 0 && (
                <Space wrap size={6} style={{ marginBottom: 12 }}>
                  {active.files.map((f) => (
                    <FileChip key={f.fileId} file={f} onClick={() => openPreview(f)} />
                  ))}
                </Space>
              )}
              {active.activity.length > 0 && (
                <Space direction="vertical" size={2} style={{ marginBottom: 12 }}>
                  {mergedActivity(active.activity).map((entry, i) =>
                    entry.kind === "fallback" ? (
                      <FallbackNotice key={i} />
                    ) : (
                      <ActivityLine key={i} entry={entry} />
                    ),
                  )}
                </Space>
              )}
              {active.status === "running" && active.text === "" && active.activity.length === 0 ? (
                <ThinkingIndicator />
              ) : (
                <Text>{active.text}</Text>
              )}
              {active.status === "error" && (
                <Alert
                  type="error"
                  showIcon
                  style={{ marginTop: 12 }}
                  message="The agent could not finish this investigation."
                  description={active.error}
                />
              )}
            </>
          )}
        </div>
        {submitError && (
          <Alert type="error" showIcon closable message={submitError} style={{ marginBottom: 8 }} />
        )}
        {uploadError && (
          <Alert
            type="error"
            showIcon
            closable
            message={uploadError}
            style={{ marginBottom: 8 }}
            onClose={() => setUploadError(null)}
          />
        )}
        {stagedFiles.length > 0 && (
          <Space wrap size={6} style={{ marginBottom: 8 }}>
            {stagedFiles.map((f) => (
              <FileChip
                key={f.fileId}
                file={f}
                onClick={() => openPreview(f)}
                onRemove={() =>
                  setStagedFiles((prev) => prev.filter((s) => s.fileId !== f.fileId))
                }
              />
            ))}
          </Space>
        )}
        <Space.Compact style={{ width: "100%" }}>
          <input
            ref={fileInputRef}
            type="file"
            multiple
            accept=".json,application/json,.csv,text/csv"
            style={{ display: "none" }}
            onChange={(e) => {
              void handleFilesSelected(e.target.files);
              e.target.value = "";
            }}
          />
          <Button
            onClick={() => fileInputRef.current?.click()}
            aria-label="Attach a file"
            title={COPY.attachTitle}
          >
            <PaperClipOutlined />
          </Button>
          <Input
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onPressEnter={handleAsk}
            placeholder={COPY.placeholder}
            prefix={<RobotOutlined />}
          />
          <Button type="primary" onClick={handleAsk} aria-label="Ask">
            <SendOutlined />
          </Button>
        </Space.Compact>
      </Content>
      <Modal
        open={previewFile !== null}
        title={previewFile?.name}
        onCancel={() => setPreviewFile(null)}
        footer={null}
        width={720}
      >
        {previewLoading ? (
          <Spin />
        ) : previewError ? (
          <Alert type="error" showIcon message={previewError} />
        ) : (
          <pre
            style={{
              maxHeight: "60vh",
              overflow: "auto",
              fontSize: 12,
              background: "rgba(127,127,127,0.08)",
              padding: 12,
              borderRadius: 6,
            }}
          >
            {formatPreview(previewFile)}
          </pre>
        )}
      </Modal>
    </Layout>
  );
}

function formatPreview(file: FileContent | null): string {
  if (!file) return "";
  try {
    return JSON.stringify(JSON.parse(file.content), null, 2);
  } catch {
    return file.content;
  }
}

function FileChip({
  file,
  onClick,
  onRemove,
}: {
  file: UploadedFile;
  onClick: () => void;
  onRemove?: () => void;
}) {
  return (
    <Tag
      style={{ cursor: "pointer", display: "inline-flex", alignItems: "center", gap: 4 }}
      onClick={onClick}
    >
      <FileTextOutlined />
      {file.name}
      {onRemove && (
        <CloseCircleFilled
          style={{ fontSize: 12, opacity: 0.6 }}
          onClick={(e) => {
            e.stopPropagation();
            onRemove();
          }}
        />
      )}
    </Tag>
  );
}

function ThinkingIndicator() {
  const [labelIndex, setLabelIndex] = useState(0);
  useEffect(() => {
    const id = setInterval(
      () => setLabelIndex((i) => (i + 1) % THINKING_LABELS.length),
      THINKING_LABEL_INTERVAL_MS,
    );
    return () => clearInterval(id);
  }, []);
  const label = THINKING_LABELS[labelIndex % THINKING_LABELS.length] ?? "Thinking";
  return <span className="gowl-thinking-label">{`${label}…`}</span>;
}

function FallbackNotice() {
  return (
    <Space size={6}>
      <SwapOutlined style={{ fontSize: 12, color: "#faad14" }} />
      <Text type="secondary" style={{ fontSize: 12 }}>
        {COPY.fallbackNotice}
      </Text>
    </Space>
  );
}

function StatusDot({ status }: { status: ThreadStatus }) {
  const color = status === "running" ? "gold" : status === "done" ? "green" : "red";
  return <Tag color={color} style={{ width: 8, height: 8, padding: 0, borderRadius: "50%" }} />;
}

interface ToolEntry {
  kind: "tool";
  tool: string;
  ok: boolean | null; // null = call made, result not back yet
}

interface FallbackEntry {
  kind: "fallback";
}

type ActivityEntry = ToolEntry | FallbackEntry;

/** Pairs each `tool_call` with its later `tool_result` by tool name,
 *  FIFO per name (two calls to the same tool resolve in the order they
 *  were made) — turns the flat event log into one line per tool
 *  invocation, the shape Cursor/Claude/ChatGPT's own "Using X…" / "✓ X"
 *  indicators render. `model_fallback` events pass through as their own
 *  entry kind rather than being forced into the tool-pairing logic
 *  above — they never had a "call" half to pair with. */
function mergedActivity(activity: ToolActivity[]): ActivityEntry[] {
  const entries: ActivityEntry[] = [];
  const pending: Record<string, number[]> = {};
  for (const item of activity) {
    if (item.phase === "tool_call") {
      entries.push({ kind: "tool", tool: item.tool, ok: null });
      (pending[item.tool] ??= []).push(entries.length - 1);
    } else if (item.phase === "tool_result") {
      const idx = pending[item.tool]?.shift();
      const existing = idx !== undefined ? entries[idx] : undefined;
      if (idx !== undefined && existing !== undefined && existing.kind === "tool") {
        entries[idx] = { kind: "tool", tool: existing.tool, ok: item.ok };
      }
    } else {
      entries.push({ kind: "fallback" });
    }
  }
  return entries;
}

function ActivityLine({ entry }: { entry: ToolEntry }) {
  return (
    <Space size={6}>
      {entry.ok === null ? (
        <LoadingOutlined spin style={{ fontSize: 12 }} />
      ) : entry.ok ? (
        <CheckOutlined style={{ fontSize: 12, color: "#52c41a" }} />
      ) : (
        <Text type="danger" style={{ fontSize: 12 }}>
          {COPY.toolFailed}
        </Text>
      )}
      <Text type="secondary" style={{ fontSize: 12 }}>
        {entry.ok === null ? "Using " : "Used "}
        <Text code style={{ fontSize: 12 }}>
          {entry.tool}
        </Text>
      </Text>
    </Space>
  );
}
