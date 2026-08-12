/** A small, standalone client for the agent service — deliberately not
 *  routed through `api.ts`'s `request()`/`BASE`. That wrapper's `BASE` is
 *  graph-owl-server's own origin; the agent service is a genuinely
 *  different backend (a separate Python/LangGraph process,
 *  `integrations/langchain/agent_service/`, never ported into
 *  graph-owl-server itself — see that service's own `streaming.py`
 *  docstring for why). `auth/index.tsx` already establishes the pattern
 *  of a direct `fetch` to a different origin for exactly this reason (its
 *  own calls to the OIDC provider) — this follows that precedent rather
 *  than shoehorning a second backend into `api.ts`'s single `BASE`.
 *
 *  The console's own signed-in access token travels with every call
 *  (`getAccessToken()`), so an investigation runs as that specific user —
 *  the agent service threads it straight through to graph-owl-server, see
 *  its own `server.py`'s `ask` handler. */

import { getAccessToken } from "../../auth";

const AGENT_SERVICE_URL: string =
  (import.meta.env.VITE_AGENT_SERVICE_URL as string | undefined) ?? "http://localhost:8899";

function authHeaders(): HeadersInit {
  const token = getAccessToken();
  return token ? { authorization: `Bearer ${token}` } : {};
}

export interface AskResult {
  threadId: string;
}

/** Submits a question and returns immediately once the agent service has
 *  scheduled the run — it does not wait for an answer. This is the whole
 *  mechanism behind "ask a second question while the first still runs":
 *  the caller gets a `threadId` back fast, opens a stream for it, and can
 *  call `askQuestion` again right away for a second question with its own
 *  independent `threadId`.
 *
 *  `fileIds` names files already uploaded via `uploadFile` — the server
 *  turns them into an explicit "here are the file IDs you can use"
 *  note ahead of the question text (see `server.py`'s
 *  `_files_context_note`) and offers the `reconcile_uploaded_files`
 *  tool only when at least one is attached. */
export async function askQuestion(
  question: string,
  fileIds: string[] = [],
): Promise<AskResult> {
  const response = await fetch(`${AGENT_SERVICE_URL}/questions`, {
    method: "POST",
    headers: { "content-type": "application/json", ...authHeaders() },
    body: JSON.stringify({ question, fileIds }),
  });
  if (!response.ok) {
    const detail = await response.text().catch(() => "");
    throw new Error(`agent service refused the question (${response.status}): ${detail}`);
  }
  return (await response.json()) as AskResult;
}

export interface UploadedFile {
  fileId: string;
  name: string;
  contentType: string;
  size: number;
}

/** Uploads a file's raw text content and returns its assigned ID —
 *  attach that ID to a subsequent `askQuestion` call, or pass it to
 *  `readFile` to preview what was actually stored. */
export async function uploadFile(name: string, contentType: string, content: string): Promise<UploadedFile> {
  const response = await fetch(`${AGENT_SERVICE_URL}/files`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ name, contentType, content }),
  });
  if (!response.ok) {
    const detail = await response.text().catch(() => "");
    throw new Error(`could not upload ${name} (${response.status}): ${detail}`);
  }
  return (await response.json()) as UploadedFile;
}

export interface FileContent extends UploadedFile {
  content: string;
}

/** Reads back a previously uploaded file's content — the preview modal's
 *  only data source, so what a user sees on click is exactly what the
 *  agent's tools would read for the same file ID. */
export async function readFile(fileId: string): Promise<FileContent> {
  const response = await fetch(`${AGENT_SERVICE_URL}/files/${fileId}`);
  if (!response.ok) {
    const detail = await response.text().catch(() => "");
    throw new Error(`could not read file ${fileId} (${response.status}): ${detail}`);
  }
  return (await response.json()) as FileContent;
}

export type ToolActivity =
  | { phase: "tool_call"; tool: string; args: Record<string, unknown> }
  | { phase: "tool_result"; tool: string; ok: boolean }
  | { phase: "model_fallback"; reason: string };

export type StreamEvent =
  | { kind: "message"; text: string }
  | { kind: "update"; data: ToolActivity }
  | { kind: "done"; status: "done" | "error"; error: string | null };

/** Opens one SSE connection for one thread. Returns a closer function
 *  rather than the raw `EventSource`, so a caller never has to remember
 *  the two different ways to tear one down (`close()` on success,
 *  `close()` again on error) — there is exactly one way to stop
 *  listening, call the returned function. */
export function streamAnswer(
  threadId: string,
  onEvent: (event: StreamEvent) => void,
): () => void {
  const source = new EventSource(`${AGENT_SERVICE_URL}/questions/${threadId}/stream`);
  source.onmessage = (event: MessageEvent<string>) => {
    const payload = JSON.parse(event.data) as StreamEvent;
    onEvent(payload);
    if (payload.kind === "done") source.close();
  };
  source.onerror = () => {
    onEvent({ kind: "done", status: "error", error: "connection lost" });
    source.close();
  };
  return () => source.close();
}
