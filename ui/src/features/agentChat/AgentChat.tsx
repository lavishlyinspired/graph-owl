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
import { Alert, Button, Empty, Input, Layout, List, Select, Space, Tag, Typography } from "antd";
import { CheckOutlined, LoadingOutlined, RobotOutlined, SendOutlined } from "@ant-design/icons";
import { askQuestion, streamAnswer, type ToolActivity } from "./agentClient";

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
}

const COPY = {
  title: "Agent",
  intro:
    "Ask the reconciliation agent a question. You can ask another one immediately — it runs alongside this one, not after it.",
  placeholder: "Ask the Reconciliation Agent…",
  emptyTranscript: "Ask a question below.",
  emptySidebar: "No questions yet.",
  submitError: "Could not reach the agent service.",
  thinking: "…",
};

export function AgentChat() {
  const [agentId, setAgentId] = useState<string>(AGENTS[0]?.id ?? "reconciliation");
  const [threads, setThreads] = useState<Record<string, ThreadState>>({});
  const [activeThreadId, setActiveThreadId] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [submitError, setSubmitError] = useState<string | null>(null);

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

  const handleAsk = useCallback(async () => {
    const question = input.trim();
    if (!question) return;
    setInput("");
    setSubmitError(null);

    let threadId: string;
    try {
      ({ threadId } = await askQuestion(question));
    } catch (e) {
      setSubmitError(e instanceof Error ? e.message : COPY.submitError);
      return;
    }

    setThreads((prev) => ({
      ...prev,
      [threadId]: { agentId, question, status: "running", text: "", activity: [], error: null },
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
  }, [input, agentId, updateThread]);

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
              {active.activity.length > 0 && (
                <Space direction="vertical" size={2} style={{ marginBottom: 12 }}>
                  {mergedActivity(active.activity).map((entry, i) => (
                    <ActivityLine key={i} entry={entry} />
                  ))}
                </Space>
              )}
              <Text>
                {active.text ||
                  (active.status === "running" && active.activity.length === 0
                    ? COPY.thinking
                    : "")}
              </Text>
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
        <Space.Compact style={{ width: "100%" }}>
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
    </Layout>
  );
}

function StatusDot({ status }: { status: ThreadStatus }) {
  const color = status === "running" ? "gold" : status === "done" ? "green" : "red";
  return <Tag color={color} style={{ width: 8, height: 8, padding: 0, borderRadius: "50%" }} />;
}

interface ActivityEntry {
  tool: string;
  ok: boolean | null; // null = call made, result not back yet
}

/** Pairs each `tool_call` with its later `tool_result` by tool name,
 *  FIFO per name (two calls to the same tool resolve in the order they
 *  were made) — turns the flat event log into one line per tool
 *  invocation, the shape Cursor/Claude/ChatGPT's own "Using X…" / "✓ X"
 *  indicators render. */
function mergedActivity(activity: ToolActivity[]): ActivityEntry[] {
  const entries: ActivityEntry[] = [];
  const pending: Record<string, number[]> = {};
  for (const item of activity) {
    if (item.phase === "tool_call") {
      entries.push({ tool: item.tool, ok: null });
      (pending[item.tool] ??= []).push(entries.length - 1);
    } else {
      const idx = pending[item.tool]?.shift();
      const existing = idx !== undefined ? entries[idx] : undefined;
      if (idx !== undefined && existing !== undefined) {
        entries[idx] = { tool: existing.tool, ok: item.ok };
      }
    }
  }
  return entries;
}

function ActivityLine({ entry }: { entry: ActivityEntry }) {
  return (
    <Space size={6}>
      {entry.ok === null ? (
        <LoadingOutlined spin style={{ fontSize: 12 }} />
      ) : entry.ok ? (
        <CheckOutlined style={{ fontSize: 12, color: "#52c41a" }} />
      ) : (
        <Text type="danger" style={{ fontSize: 12 }}>
          ✕
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
