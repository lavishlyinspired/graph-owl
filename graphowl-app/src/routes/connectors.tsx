import { useState } from "react";
import { runPostgresConnector, testConnector, type ConnectionTestResult, type ConnectorRun } from "../lib/api";
import { strings } from "../lib/strings";

/** Only "postgres" is a real, registered connector type at the HTTP layer
 *  (`/connectors/{connector}/schema` and `/test` both 404 on anything
 *  else) — matching `CLAUDE.md`'s "source connectors: one crate, module
 *  per connector" convention, this stays a hardcoded list of one rather
 *  than a fake multi-connector catalog. */
export default function ConnectorsRoute() {
  const [host, setHost] = useState("");
  const [port, setPort] = useState("5432");
  const [database, setDatabase] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [serviceName, setServiceName] = useState("");
  const [testResult, setTestResult] = useState<ConnectionTestResult | null>(null);
  const [runResult, setRunResult] = useState<ConnectorRun | null>(null);
  const [busy, setBusy] = useState(false);

  const settings = () => ({ host, port: Number(port) || 5432, database, username });
  const connectionString = () => `postgres://${username}:${password}@${host}:${port || "5432"}/${database}`;

  const complete = host.trim() && database.trim() && username.trim();

  const runTest = async () => {
    setBusy(true);
    try {
      const result = await testConnector("postgres", settings(), password);
      setTestResult(result);
    } finally {
      setBusy(false);
    }
  };

  const runSync = async () => {
    if (!serviceName.trim()) return;
    setBusy(true);
    try {
      const run = await runPostgresConnector({ connectionString: connectionString(), serviceName: serviceName.trim() });
      setRunResult(run);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="p-8">
      <h1 className="mb-1 text-[25px] font-semibold text-gowl-t1">{strings.connectorsTitle}</h1>
      <p className="mb-5 text-[16.5px] text-gowl-t5">{strings.connectorsDescription}</p>

      <div className="max-w-[520px] rounded-lg border border-gowl-line bg-gowl-panel p-5">
        <div className="mb-4">
          <div className="text-[18px] font-semibold text-gowl-t1">{strings.connectorsPostgresName}</div>
          <div className="text-[16px] text-gowl-t5">{strings.connectorsPostgresDescription}</div>
        </div>

        <div className="mb-3 grid grid-cols-2 gap-2">
          <input
            value={host}
            onChange={(e) => setHost(e.target.value)}
            placeholder="Host"
            className="rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
          />
          <input
            value={port}
            onChange={(e) => setPort(e.target.value)}
            placeholder="Port"
            className="rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
          />
          <input
            value={database}
            onChange={(e) => setDatabase(e.target.value)}
            placeholder="Database"
            className="rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
          />
          <input
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            placeholder="Username"
            className="rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
          />
          <input
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            type="password"
            placeholder="Password"
            className="col-span-2 rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
          />
        </div>

        <button
          type="button"
          disabled={busy || !complete}
          onClick={runTest}
          className="mb-2 rounded-md border border-gowl-line-2 px-3 py-1.5 text-[16px] text-gowl-t2 disabled:opacity-40"
        >
          {strings.connectorsTestSubmit}
        </button>
        {testResult && (
          <p className={`mb-3 text-[16px] ${testResult.ok ? "text-gowl-ok" : "text-gowl-bad"}`}>
            {testResult.ok ? strings.connectorsTestOk : testResult.detail}
          </p>
        )}

        <div className="mt-3 border-t border-gowl-line-2 pt-3">
          <div className="mb-2 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.connectorsRunTitle}</div>
          <input
            value={serviceName}
            onChange={(e) => setServiceName(e.target.value)}
            placeholder={strings.connectorsServiceName}
            className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2 py-1.5 text-[16px] text-gowl-t1"
          />
          <button
            type="button"
            disabled={busy || !complete || serviceName.trim().length === 0}
            onClick={runSync}
            className="rounded-md bg-gowl-accent px-3 py-1.5 text-[16px] font-semibold text-gowl-accent-on disabled:opacity-40"
          >
            {strings.connectorsRunSubmit}
          </button>
          {runResult && (
            <div className="mt-3 rounded-md border border-gowl-line-2 bg-gowl-panel-2 p-3 font-mono text-[15.5px] text-gowl-t2">
              <div className="mb-1 font-mono text-[13.5px] tracking-widest text-gowl-t6">{strings.connectorsRunResult}</div>
              {strings.runPrefixCreated}
              {runResult.created} {strings.runPrefixSkipped}
              {runResult.skipped}{" "}
              {runResult.failed > 0 && (
                <span className="text-gowl-bad">
                  {strings.runPrefixFailed}
                  {runResult.failed}
                </span>
              )}{" "}
              {strings.runPrefixDeleted}
              {runResult.deleted}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
