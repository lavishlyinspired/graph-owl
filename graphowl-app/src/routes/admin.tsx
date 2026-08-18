import { useEffect, useState } from "react";
import {
  deleteTeam,
  fetchHealth,
  fetchMapping,
  fetchTeams,
  setUserRoles,
  upsertTeam,
  upsertUser,
  type Team,
} from "../lib/api";
import { strings } from "../lib/strings";

const TABS = ["teams", "users", "webhooks", "health", "budgets"] as const;
type Tab = (typeof TABS)[number];

const TAB_LABEL: Record<Tab, string> = {
  teams: strings.adminTabTeams,
  users: strings.adminTabUsers,
  webhooks: strings.adminTabWebhooks,
  health: strings.adminTabHealth,
  budgets: strings.adminTabBudgets,
};

function TeamsPanel() {
  const [teams, setTeams] = useState<readonly Team[] | null>(null);
  const [id, setId] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [busy, setBusy] = useState(false);

  const load = () => {
    fetchTeams().then(setTeams);
  };

  useEffect(load, []);

  if (!teams) return <div className="text-[13px] text-gowl-t5">{strings.studioLoading}</div>;

  const runCreate = async () => {
    if (id.trim().length === 0 || displayName.trim().length === 0) return;
    setBusy(true);
    try {
      await upsertTeam({ id: id.trim(), displayName: displayName.trim() });
      setId("");
      setDisplayName("");
      load();
    } finally {
      setBusy(false);
    }
  };

  const runDelete = async (team: Team) => {
    setBusy(true);
    try {
      await deleteTeam(team.id);
      load();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      <div className="mb-3 flex gap-2">
        <input
          value={id}
          onChange={(e) => setId(e.target.value)}
          placeholder={strings.adminTeamId}
          className="rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 font-mono text-[12px] text-gowl-t1"
        />
        <input
          value={displayName}
          onChange={(e) => setDisplayName(e.target.value)}
          placeholder={strings.adminTeamName}
          className="rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 text-[12px] text-gowl-t1"
        />
        <button
          type="button"
          disabled={busy || id.trim().length === 0 || displayName.trim().length === 0}
          onClick={runCreate}
          className="rounded-md bg-gowl-accent px-3 py-1.5 text-[12px] font-semibold text-gowl-accent-on disabled:opacity-40"
        >
          {strings.adminNewTeam}
        </button>
      </div>
      <div className="overflow-hidden rounded-lg border border-gowl-line bg-gowl-panel">
        {teams.length === 0 ? (
          <div className="p-6 text-[12.5px] text-gowl-t5">{strings.adminTeamsEmpty}</div>
        ) : (
          teams.map((team) => (
            <div key={team.id} className="flex items-center justify-between border-b border-gowl-row px-4 py-2.5 last:border-b-0">
              <div>
                <div className="text-[13px] text-gowl-t1">{team.displayName}</div>
                <div className="font-mono text-[11px] text-gowl-t6">{team.id}</div>
              </div>
              <button type="button" disabled={busy} onClick={() => runDelete(team)} className="text-[12px] text-gowl-bad">
                {strings.adminDeleteTeam}
              </button>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

function UsersPanel() {
  const [userId, setUserId] = useState("");
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [roles, setRoles] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const runSave = async () => {
    if (userId.trim().length === 0 || name.trim().length === 0) return;
    setBusy(true);
    setMessage(null);
    try {
      await upsertUser(userId.trim(), name.trim(), email.trim() || undefined);
      setMessage(strings.adminSaveUser);
    } finally {
      setBusy(false);
    }
  };

  const runRoles = async () => {
    if (userId.trim().length === 0) return;
    setBusy(true);
    setMessage(null);
    try {
      await setUserRoles(
        userId.trim(),
        roles.split(",").map((r) => r.trim()).filter((r) => r.length > 0),
      );
      setMessage(strings.adminSetRoles);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="max-w-[480px]">
      <p className="mb-4 text-[12.5px] text-gowl-t5">{strings.adminUserGapNote}</p>
      <input
        value={userId}
        onChange={(e) => setUserId(e.target.value)}
        placeholder={strings.adminUserId}
        className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 font-mono text-[12px] text-gowl-t1"
      />
      <input
        value={name}
        onChange={(e) => setName(e.target.value)}
        placeholder={strings.adminUserName}
        className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 text-[12px] text-gowl-t1"
      />
      <input
        value={email}
        onChange={(e) => setEmail(e.target.value)}
        placeholder={strings.adminUserEmail}
        className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 text-[12px] text-gowl-t1"
      />
      <button
        type="button"
        disabled={busy}
        onClick={runSave}
        className="mb-4 rounded-md bg-gowl-accent px-3 py-1.5 text-[12px] font-semibold text-gowl-accent-on disabled:opacity-40"
      >
        {strings.adminSaveUser}
      </button>

      <input
        value={roles}
        onChange={(e) => setRoles(e.target.value)}
        placeholder={strings.adminUserRoles}
        className="mb-2 w-full rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 text-[12px] text-gowl-t1"
      />
      <button
        type="button"
        disabled={busy}
        onClick={runRoles}
        className="rounded-md border border-gowl-line-2 px-3 py-1.5 text-[12px] text-gowl-t2 disabled:opacity-40"
      >
        {strings.adminSetRoles}
      </button>
      {message && <p className="mt-3 text-[12px] text-gowl-ok">{message}</p>}
    </div>
  );
}

function WebhooksPanel() {
  const [name, setName] = useState("");
  const [result, setResult] = useState<unknown>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const runLookup = async () => {
    if (name.trim().length === 0) return;
    setBusy(true);
    setError(null);
    try {
      const mapping = await fetchMapping(name.trim());
      setResult(mapping);
    } catch {
      setError(strings.studioError);
      setResult(null);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="max-w-[560px]">
      <p className="mb-4 text-[12.5px] text-gowl-t5">{strings.adminWebhookGapNote}</p>
      <div className="mb-3 flex gap-2">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={strings.adminWebhookName}
          className="flex-1 rounded-md border border-gowl-line-2 bg-gowl-input px-2.5 py-1.5 font-mono text-[12px] text-gowl-t1"
        />
        <button
          type="button"
          disabled={busy || name.trim().length === 0}
          onClick={runLookup}
          className="rounded-md bg-gowl-accent px-3 py-1.5 text-[12px] font-semibold text-gowl-accent-on disabled:opacity-40"
        >
          {strings.adminWebhookLookup}
        </button>
      </div>
      {error && <p className="text-[12.5px] text-gowl-bad">{error}</p>}
      {result != null && (
        <pre className="overflow-x-auto rounded-md border border-gowl-line-2 bg-gowl-panel-2 p-3 font-mono text-[11px] text-gowl-t2">
          {JSON.stringify(result, null, 2)}
        </pre>
      )}
    </div>
  );
}

function HealthPanel() {
  const [health, setHealth] = useState<{ readonly status: string; readonly version: string } | null>(null);

  useEffect(() => {
    fetchHealth().then(setHealth);
  }, []);

  if (!health) return <div className="text-[13px] text-gowl-t5">{strings.studioLoading}</div>;

  return (
    <div className="flex gap-6">
      <div>
        <div className="mb-1 font-mono text-[9.5px] tracking-widest text-gowl-t6">{strings.adminHealthStatus}</div>
        <div className="font-mono text-[16px] text-gowl-ok">{health.status}</div>
      </div>
      <div>
        <div className="mb-1 font-mono text-[9.5px] tracking-widest text-gowl-t6">{strings.adminHealthVersion}</div>
        <div className="font-mono text-[16px] text-gowl-t1">{health.version}</div>
      </div>
    </div>
  );
}

export default function AdminRoute() {
  const [tab, setTab] = useState<Tab>("teams");

  return (
    <div className="p-8">
      <h1 className="mb-1 text-[21px] font-semibold text-gowl-t1">{strings.adminTitle}</h1>
      <p className="mb-5 text-[12.5px] text-gowl-t5">{strings.adminDescription}</p>

      <div className="mb-4 flex gap-1 border-b border-gowl-line">
        {TABS.map((t) => (
          <button
            key={t}
            type="button"
            onClick={() => setTab(t)}
            className={`px-3 py-2 text-[12.5px] ${
              tab === t ? "border-b-2 border-gowl-accent text-gowl-accent" : "text-gowl-t5 hover:text-gowl-t2"
            }`}
          >
            {TAB_LABEL[t]}
          </button>
        ))}
      </div>

      {tab === "teams" && <TeamsPanel />}
      {tab === "users" && <UsersPanel />}
      {tab === "webhooks" && <WebhooksPanel />}
      {tab === "health" && <HealthPanel />}
      {tab === "budgets" && <p className="text-[13px] text-gowl-t5">{strings.adminBudgetsGapNote}</p>}
    </div>
  );
}
