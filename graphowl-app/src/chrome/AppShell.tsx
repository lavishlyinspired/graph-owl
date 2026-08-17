import { useEffect, useState } from "react";
import { Outlet } from "react-router-dom";
import { TopBar } from "./TopBar";
import { Rail } from "./Rail";
import { InboxDrawer, type InboxItem } from "./InboxDrawer";
import { applyTheme, persistTheme, resolveInitialTheme, toggleTheme, type ThemeMode } from "../lib/theme";

/** Plan 122a A0: the chrome present on every one of the 24 routes. Inbox
 *  data is empty here — real aggregation lands in A1 (`GET /inbox`). */
export function AppShell() {
  const [theme, setTheme] = useState<ThemeMode>(() => resolveInitialTheme());
  const [railOpen, setRailOpen] = useState(true);
  const [inboxOpen, setInboxOpen] = useState(false);
  const [items] = useState<readonly InboxItem[]>([]);

  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  const handleToggleTheme = () => {
    const next = toggleTheme(theme);
    setTheme(next);
    persistTheme(next);
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
      />
      <div className="flex min-h-0 flex-1">
        <Rail open={railOpen} onToggle={() => setRailOpen((v) => !v)} />
        <main className="min-w-0 flex-1 overflow-y-auto">
          <Outlet />
        </main>
      </div>
      <InboxDrawer
        open={inboxOpen}
        items={items}
        onClose={() => setInboxOpen(false)}
        onApprove={() => setInboxOpen(false)}
        onReject={() => setInboxOpen(false)}
      />
    </div>
  );
}
