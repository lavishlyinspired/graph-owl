import { strings } from "../lib/strings";
import { cn } from "../lib/cn";
import type { ThemeMode } from "../lib/theme";
import { SearchBox } from "./SearchBox";
import { WorkspaceSwitcher } from "./WorkspaceSwitcher";

interface TopBarProps {
  readonly theme: ThemeMode;
  readonly onToggleTheme: () => void;
  readonly onOpenInbox: () => void;
  readonly pendingCount: number;
  readonly asOf: string;
  /** From `GET /me` once A1 wires it — a placeholder avatar is display data,
   *  not copy, so it is a prop rather than a `strings.ts` entry. */
  readonly userInitials: string;
  readonly workspaceName: string;
}

export function TopBar({ theme, onToggleTheme, onOpenInbox, pendingCount, asOf, userInitials, workspaceName }: TopBarProps) {
  return (
    <div className="flex h-[52px] flex-none items-center gap-5 border-b border-gowl-line bg-gowl-panel px-4.5">
      <div className="flex items-center gap-2.5">
        <div className="relative h-5 w-5 rounded-md bg-gowl-accent">
          <div className="absolute inset-1.5 rounded-sm bg-gowl-bg" />
        </div>
        <span className="text-[13px] font-semibold tracking-[0.14em]">{strings.brand}</span>
      </div>

      <WorkspaceSwitcher name={workspaceName} />

      <SearchBox />

      <button
        type="button"
        onClick={onToggleTheme}
        className="rounded-md border border-gowl-line px-2.5 py-1 font-mono text-[10px] tracking-widest text-gowl-t5 hover:border-gowl-line-2"
      >
        {theme === "dark" ? strings.themeNight : strings.themeDay}
      </button>

      <div className="font-mono text-[11px] text-gowl-t5">{asOf}</div>

      <button
        type="button"
        onClick={onOpenInbox}
        aria-label={strings.inboxTitle}
        className={cn(
          "relative rounded-md border border-gowl-line px-2.5 py-1.5 text-gowl-t4 hover:border-gowl-line-2",
          pendingCount > 0 && "border-gowl-amber text-gowl-amber",
        )}
      >
        <span aria-hidden="true">{strings.inboxIcon}</span>
        {pendingCount > 0 && (
          <span className="ml-1.5 rounded-full bg-gowl-amber-bg px-1.5 py-0.5 font-mono text-[10px] text-gowl-amber">
            {pendingCount}
          </span>
        )}
      </button>

      <div className="flex h-7 w-7 items-center justify-center rounded-full bg-gowl-avatar font-mono text-[11px] text-gowl-t2">
        {userInitials}
      </div>
    </div>
  );
}
