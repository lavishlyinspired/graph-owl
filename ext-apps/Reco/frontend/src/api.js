const BASE = "/api";

async function request(path, options = {}) {
  const res = await fetch(`${BASE}${path}`, options);
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(text || `${res.status} ${res.statusText}`);
  }
  return res;
}

export const api = {
  health: () => request("/health").then((r) => r.json()),
  sample: () => request("/sample", { method: "POST" }).then((r) => r.json()),
  reset: () => request("/reset", { method: "POST" }).then((r) => r.json()),
  overview: () => request("/overview").then((r) => r.json()),
  saveMapping: (payload) =>
    request("/mapping", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    }).then((r) => r.json()),
  reconcile: () => request("/reconcile", { method: "POST" }).then((r) => r.json()),
  followUps: () => request("/act/follow-ups", { method: "POST" }).then((r) => r.json()),
  report: () => request("/act/report", { method: "POST" }).then((r) => r.json()),
  aiJob: (id) => request(`/ai/jobs/${id}`).then((r) => r.json()),
  aiSummary: () => request("/act/summary").then((r) => r.json()),
  upload: async (files) => {
    const form = new FormData();
    for (const file of files) form.append("files", file);
    return fetch(`${BASE}/upload`, { method: "POST", body: form }).then((r) => r.json());
  },
  download: (path) => {
    const url = `${BASE}${path}`;
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = "";
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
  },
};

export async function pollJob(jobId, { interval = 5000, onProgress } = {}) {
  for (;;) {
    const res = await api.aiJob(jobId);
    if (res.status === "done") return res.result;
    if (res.status === "error") throw new Error(res.error || "AI job failed");
    onProgress?.(res);
    await new Promise((resolve) => setTimeout(resolve, interval));
  }
}
