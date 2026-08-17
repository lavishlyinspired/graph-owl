import { Suspense, useCallback, useEffect, useState } from "react";
import { Outlet } from "react-router-dom";
import { TopBar } from "./TopBar";
import { Rail } from "./Rail";
import { InboxDrawer } from "./InboxDrawer";
import { fetchInbox, performInboxAction, resolveInboxAction, type InboxItem } from "../lib/api";
import { applyTheme, persistTheme, resolveInitialTheme, toggleTheme, type ThemeMode } from "../lib/theme";

const DEFAULT_WORKSPACE_NAME = "default";

/** Plan 122a A0 shell + A1 real data: the chrome present on every one of
 *  the 24 routes, now backed by `GET /inbox` rather than an empty
 *  placeholder. */
export function AppShell() {
  const [theme, setTheme] = useState<ThemeMode>(() => resolveInitialTheme());
  const [railOpen, setRailOpen] = useState(true);
  const [inboxOpen, setInboxOpen] = useState(false);
  const [items, setItems] = useState<readonly InboxItem[]>([]);

  const refreshInbox = useCallback(() => {
    fetchInbox()
      .then((response) => setItems(response.items))
      .catch(() => setItems([]));
  }, []);

  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  useEffect(() => {
    refreshInbox();
  }, [refreshInbox]);

  const handleToggleTheme = () => {
    const next = toggleTheme(theme);
    setTheme(next);
    persistTheme(next);
  };

  const handleDecide = async (item: InboxItem, decision: "approve" | "reject", reason?: string) => {
    const action = resolveInboxAction(item.source, item.id, decision, reason);
    await performInboxAction(action);
    refreshInbox();
  };

  return (
    <div className="flex h-screen flex-col bg-gowl-bg text-gowl-t1">
      <TopBar
        theme={theme}
        onToggleTheme={handleToggleTheme}
        onOpenInbox={() => setInboxOpen(true)}
        pendingCount={items.length}
        asOf="AS OF —"
        userInitials="—"
        workspaceName={DEFAULT_WORKSPACE_NAME}
      />
      <div className="flex min-h-0 flex-1">
        <Rail open={railOpen} onToggle={() => setRailOpen((v) => !v)} />
        <main className="min-w-0 flex-1 overflow-y-auto">
          <Suspense fallback={null}>
            <Outlet />
          </Suspense>
        </main>
      </div>
      <InboxDrawer open={inboxOpen} items={items} onClose={() => setInboxOpen(false)} onDecide={handleDecide} />
    </div>
  );
}
