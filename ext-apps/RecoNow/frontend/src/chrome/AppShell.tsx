import { Suspense, useEffect, useState } from "react";
import { Outlet, useLocation } from "react-router-dom";
import { TopBar } from "./TopBar";
import { Rail } from "./Rail";
import { AskPanel } from "./AskPanel";
import { InboxDrawer } from "./InboxDrawer";
import { fetchApprovals, fetchClients, fetchPeriods } from "../lib/api";
import { EMPTY_WORKSPACE, loadWorkspace, persistWorkspace, selectClient, selectPeriod } from "../lib/workspace";
import { NAV } from "../lib/nav";
import { strings } from "../lib/strings";

export function AppShell() {
  const location = useLocation();
  const [workspace, setWorkspace] = useState(loadWorkspace);
  const [askOpen, setAskOpen] = useState(false);
  const [inboxOpen, setInboxOpen] = useState(false);
  const [pendingCount, setPendingCount] = useState(0);
  const [pendingVersion, setPendingVersion] = useState(0);

  useEffect(() => persistWorkspace(workspace), [workspace]);

  // A workspace persisted to localStorage can outlive the server it was
  // saved against — found live: a client/period selected against one
  // Postgres database, still present in localStorage after switching to a
  // fresh one, satisfied every `clientId != null` check downstream while
  // pointing at nothing. Confirmed once, on mount / whenever the ids
  // change, rather than trusted forever.
  useEffect(() => {
    const clientId = workspace.clientId;
    if (!clientId) return;
    fetchClients()
      .then((clients) => {
        if (!clients.some((c) => c.id === clientId)) {
          setWorkspace(EMPTY_WORKSPACE);
          return;
        }
        const periodId = workspace.periodId;
        if (!periodId) return;
        fetchPeriods(clientId).then((periods) => {
          if (!periods.some((p) => p.id === periodId)) {
            setWorkspace((w) => (w.clientId === clientId ? selectClient(w, clientId) : w));
          }
        });
      })
      .catch(() => {
        /* transient network failure — do not evict a possibly-still-valid workspace */
      });
  }, [workspace.clientId, workspace.periodId]);

  useEffect(() => {
    if (!workspace.clientId || !workspace.periodId) {
      setPendingCount(0);
      return;
    }
    fetchApprovals(workspace.clientId, workspace.periodId)
      .then((items) => setPendingCount(items.length))
      .catch(() => setPendingCount(0));
  }, [workspace.clientId, workspace.periodId, inboxOpen, pendingVersion]);

  const pageTitle = pageTitleForPath(location.pathname);

  return (
    <div className="flex h-screen flex-col bg-reco-bg text-reco-t1">
      <TopBar
        clientId={workspace.clientId}
        periodId={workspace.periodId}
        onSelectClient={(id) => setWorkspace((w) => selectClient(w, id))}
        onSelectPeriod={(id) => setWorkspace((w) => selectPeriod(w, id))}
        onOpenAsk={() => setAskOpen((v) => !v)}
        onOpenInbox={() => setInboxOpen((v) => !v)}
        pendingCount={pendingCount}
      />

      {askOpen && workspace.clientId && workspace.periodId && (
        <AskPanel clientId={workspace.clientId} periodId={workspace.periodId} onClose={() => setAskOpen(false)} />
      )}

      <div className="flex min-h-0 flex-1">
        <Rail />
        <main className="min-w-0 flex-1 overflow-y-auto">
          <h1 className="sr-only">{pageTitle}</h1>
          <Suspense fallback={null}>
            <Outlet context={workspace} />
          </Suspense>
        </main>
      </div>

      {inboxOpen && workspace.clientId && workspace.periodId && (
        <InboxDrawer
          clientId={workspace.clientId}
          periodId={workspace.periodId}
          onClose={() => setInboxOpen(false)}
          onDecided={() => setPendingVersion((v) => v + 1)}
        />
      )}
    </div>
  );
}

function pageTitleForPath(pathname: string): string {
  const segment = pathname.split("?")[0]?.split("/").filter(Boolean)[0];
  const match = NAV.flatMap((g) => g.items).find((item) => item.route === segment);
  return match?.label ?? strings.brand;
}
