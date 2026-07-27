import { useCallback, useEffect, useState } from "react";
import { type Asset, type AssetKind, ApiError, api } from "./api";
import "./tokens.css";
import "./app.css";

const KIND_GLYPH: Record<AssetKind, string> = {
  service: "◈", database: "▣", schema: "▤", table: "▦", column: "▪",
};

function KindBadge({ kind }: { kind: AssetKind }) {
  return (
    <span className={`badge badge-${kind}`}>
      <span aria-hidden="true">{KIND_GLYPH[kind]}</span> {kind}
    </span>
  );
}

/** A tree node that loads its children on expand. The catalog is a hierarchy,
 *  and loading it whole would fetch every column of every table up front. */
function TreeNode({
  asset, selectedId, onSelect, depth,
}: {
  asset: Asset;
  selectedId: string | null;
  onSelect: (a: Asset) => void;
  depth: number;
}) {
  const [open, setOpen] = useState(depth < 2);
  const [children, setChildren] = useState<Asset[] | null>(null);

  useEffect(() => {
    if (open && children === null && asset.kind !== "column") {
      api.children(asset.id).then(setChildren).catch(() => setChildren([]));
    }
  }, [open, children, asset.id, asset.kind]);

  const expandable = asset.kind !== "column";
  return (
    <li>
      <div
        className={`tree-row${selectedId === asset.id ? " tree-row-selected" : ""}`}
        style={{ paddingLeft: `${depth * 14 + 8}px` }}
      >
        <button
          className="tree-toggle"
          onClick={() => setOpen((o) => !o)}
          aria-label={open ? `Collapse ${asset.name}` : `Expand ${asset.name}`}
          disabled={!expandable}
        >
          {expandable ? (open ? "▾" : "▸") : "·"}
        </button>
        <button className="tree-label" onClick={() => onSelect(asset)}>
          <span className="tree-glyph" aria-hidden="true">{KIND_GLYPH[asset.kind]}</span>
          {asset.name}
        </button>
      </div>
      {open && children && children.length > 0 && (
        <ul className="tree">
          {children.map((child) => (
            <TreeNode key={child.id} asset={child} selectedId={selectedId}
                      onSelect={onSelect} depth={depth + 1} />
          ))}
        </ul>
      )}
    </li>
  );
}

function Detail({ asset }: { asset: Asset }) {
  const [ancestors, setAncestors] = useState<Asset[]>([]);
  const [children, setChildren] = useState<Asset[]>([]);

  useEffect(() => {
    api.ancestors(asset.id).then(setAncestors).catch(() => setAncestors([]));
    if (asset.kind !== "column") {
      api.children(asset.id).then(setChildren).catch(() => setChildren([]));
    } else {
      setChildren([]);
    }
  }, [asset.id, asset.kind]);

  const properties = Object.entries(asset.properties ?? {});
  return (
    <article className="detail">
      <nav className="breadcrumb" aria-label="Breadcrumb">
        {ancestors.map((a, i) => (
          <span key={a.id}>
            {i > 0 && <span className="crumb-sep" aria-hidden="true">/</span>}
            <span className={i === ancestors.length - 1 ? "crumb-current" : "crumb"}>{a.name}</span>
          </span>
        ))}
      </nav>

      <header className="detail-head">
        <h1>{asset.name}</h1>
        <KindBadge kind={asset.kind} />
      </header>
      <p className="fqn">{asset.fullyQualifiedName}</p>

      {/* The trust bar. Nothing populates it yet — Epic 3 brings versioning,
          Epic 26 certification — so it says so rather than showing a
          confident-looking blank. 39-ui-foundation.md decision 6. */}
      <div className="trust-bar">
        <span className="trust-item trust-pending">◷ no version history yet</span>
        <span className="trust-item trust-pending">◐ confidence not scored</span>
        <span className="trust-item trust-pending">⚑ uncertified</span>
      </div>

      <p className="detail-description">
        {asset.description ?? <span className="empty-inline">No description. A connector reported this asset structurally; nobody has described it.</span>}
      </p>

      {properties.length > 0 && (
        <section>
          <h2>Properties</h2>
          <table className="props">
            <tbody>
              {properties.map(([key, value]) => (
                <tr key={key}>
                  <th scope="row">{key}</th>
                  <td><code>{String(value)}</code></td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}

      {children.length > 0 && (
        <section>
          <h2>{children[0]!.kind === "column" ? "Columns" : "Contains"} <span className="count">{children.length}</span></h2>
          <table className="children">
            <thead>
              <tr><th>Name</th><th>Kind</th><th>Type</th></tr>
            </thead>
            <tbody>
              {children.map((child) => (
                <tr key={child.id}>
                  <td>{child.name}</td>
                  <td><KindBadge kind={child.kind} /></td>
                  <td><code>{String(child.properties?.["dataType"] ?? child.properties?.["tableType"] ?? "—")}</code></td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
    </article>
  );
}

function ConnectorPanel({ onDone }: { onDone: () => void }) {
  const [connectionString, setConnectionString] = useState(
    "postgres://postgres:postgres@localhost:5432/postgres",
  );
  const [serviceName, setServiceName] = useState("warehouse");
  const [status, setStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const run = async () => {
    setBusy(true);
    setStatus(null);
    try {
      const result = await api.runPostgresConnector({ connectionString, serviceName });
      setStatus(`Catalogued ${result.created} assets${result.failed ? `, ${result.failed} failed` : ""}.`);
      onDone();
    } catch (error) {
      setStatus(error instanceof ApiError ? error.problem.detail : String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="connector">
      <h2>Catalog a Postgres source</h2>
      <label>
        Connection string
        <input value={connectionString} onChange={(e) => setConnectionString(e.target.value)} />
      </label>
      <label>
        Service name
        <input value={serviceName} onChange={(e) => setServiceName(e.target.value)} />
      </label>
      <button className="primary" onClick={run} disabled={busy}>
        {busy ? "Running…" : "Run connector"}
      </button>
      {status && <p className="status">{status}</p>}
    </div>
  );
}

export default function App() {
  const [roots, setRoots] = useState<Asset[] | null>(null);
  const [selected, setSelectedRaw] = useState<Asset | null>(null);

  // Selection lives in the URL so an asset can be pasted into a ticket.
  const setSelected = useCallback((asset: Asset | null) => {
    setSelectedRaw(asset);
    const url = asset ? `?asset=${asset.id}` : window.location.pathname;
    window.history.replaceState(null, "", url);
  }, []);

  useEffect(() => {
    const id = new URLSearchParams(window.location.search).get("asset");
    if (id) api.asset(id).then(setSelectedRaw).catch(() => undefined);
  }, []);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Asset[] | null>(null);
  const [stats, setStats] = useState<{ kind: AssetKind; count: number }[]>([]);

  const refresh = useCallback(() => {
    api.roots().then(setRoots).catch(() => setRoots([]));
    api.stats().then((s) => setStats(s.byKind)).catch(() => setStats([]));
  }, []);

  useEffect(refresh, [refresh]);

  useEffect(() => {
    if (query.trim().length < 2) { setResults(null); return; }
    const timer = setTimeout(() => {
      api.search(query).then((page) => setResults(page.data)).catch(() => setResults([]));
    }, 150);
    return () => clearTimeout(timer);
  }, [query]);

  const total = stats.reduce((sum, s) => sum + s.count, 0);

  return (
    <div className="shell">
      <header className="topbar">
        <span className="brand">◈ graph-owl</span>
        <input
          className="search"
          placeholder="Search assets…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-label="Search assets"
        />
        {/* The time control. Inert until Epic 4 lands time travel — but it
            lives in the chrome from the start, because that is where a
            session-wide property belongs. 00h-ui-design-system.md. */}
        <span className="now" title="Time travel arrives with Epic 4">◷ now</span>
      </header>

      <div className="body">
        <aside className="rail">
          <div className="rail-stats">
            {stats.map((s) => (
              <div key={s.kind} className="stat">
                <span className="stat-n">{s.count}</span>
                <span className="stat-k">{s.kind}s</span>
              </div>
            ))}
          </div>
          <h2 className="rail-title">Hierarchy</h2>
          {roots === null ? (
            <p className="muted">Loading…</p>
          ) : roots.length === 0 ? (
            <p className="muted">Nothing catalogued yet.</p>
          ) : (
            <ul className="tree">
              {roots.map((root) => (
                <TreeNode key={root.id} asset={root} selectedId={selected?.id ?? null}
                          onSelect={setSelected} depth={0} />
              ))}
            </ul>
          )}
        </aside>

        <main className="main">
          {results !== null ? (
            <section className="results">
              <h1>{results.length} result{results.length === 1 ? "" : "s"} for “{query}”</h1>
              {results.length === 0 && <p className="muted">Nothing matched.</p>}
              <ul className="result-list">
                {results.map((asset) => (
                  <li key={asset.id}>
                    <button onClick={() => { setSelected(asset); setQuery(""); }}>
                      <span className="result-name">{asset.name}</span>
                      <KindBadge kind={asset.kind} />
                      <span className="fqn">{asset.fullyQualifiedName}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          ) : selected ? (
            <Detail asset={selected} />
          ) : total === 0 ? (
            /* The empty-database first run. It is the first thing an evaluator
               sees and the last thing anyone tests — 39-ui-foundation.md
               Slice F — so it offers the next action rather than a blank page. */
            <section className="empty">
              <h1>Nothing catalogued yet</h1>
              <p>graph-owl reads a source's structure and builds a browsable hierarchy from it. Point it at a Postgres database to begin.</p>
              <ConnectorPanel onDone={refresh} />
            </section>
          ) : (
            <section className="empty">
              <h1>{total} assets catalogued</h1>
              <p>Pick something from the hierarchy, or search above.</p>
              <ConnectorPanel onDone={refresh} />
            </section>
          )}
        </main>
      </div>
    </div>
  );
}
