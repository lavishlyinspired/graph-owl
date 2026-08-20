import { NavLink } from "react-router-dom";
import { NAV } from "../lib/nav";
import { strings } from "../lib/strings";
import { cn } from "../lib/cn";

interface RailProps {
  readonly open: boolean;
  readonly onToggle: () => void;
}

export function Rail({ open, onToggle }: RailProps) {
  return (
    <nav
      aria-label="Primary"
      className={cn(
        "flex flex-none flex-col overflow-y-auto border-r border-gowl-line bg-gowl-panel-2 transition-[width]",
        open ? "w-[220px]" : "w-[56px]",
      )}
    >
      <button
        type="button"
        onClick={onToggle}
        className="border-b border-gowl-line px-3 py-2 text-left font-mono text-[14px] tracking-widest text-gowl-t6 hover:text-gowl-t4"
      >
        {open ? strings.collapseNav : "»"}
      </button>
      {NAV.map((group) => (
        <div key={group.label} className="py-2">
          {open && (
            <div className="px-3 pb-1 font-mono text-[14px] tracking-widest text-gowl-t7">{group.label}</div>
          )}
          {group.items.map((item) => (
            <NavLink
              key={item.route}
              to={`/${item.route}`}
              className={({ isActive }) =>
                cn(
                  "flex items-center gap-2.5 px-3 py-1.5 text-[17px] text-gowl-t1 hover:bg-gowl-row hover:text-gowl-t0",
                  !open && "justify-center",
                  isActive && "bg-gowl-row text-gowl-accent",
                )
              }
              title={item.label}
            >
              <item.icon className="size-[17px] shrink-0" />
              {open && <span className="truncate">{item.label}</span>}
            </NavLink>
          ))}
        </div>
      ))}
    </nav>
  );
}
