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
 *  independent `threadId`. */
export async function askQuestion(question: string): Promise<AskResult> {
  const response = await fetch(`${AGENT_SERVICE_URL}/questions`, {
    method: "POST",
    headers: { "content-type": "application/json", ...authHeaders() },
    body: JSON.stringify({ question }),
  });
  if (!response.ok) {
    const detail = await response.text().catch(() => "");
    throw new Error(`agent service refused the question (${response.status}): ${detail}`);
  }
  return (await response.json()) as AskResult;
}

export type StreamEvent =
  | { kind: "message"; text: string }
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
