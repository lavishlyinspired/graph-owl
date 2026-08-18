import { NavLink } from "react-router-dom";
import { NAV } from "../lib/nav";

/** Structure and copy read verbatim off the delivered mockup's own `nav`
 *  render (`Reco Now.dc.html`) — 9 groups, 28 destinations, collapsible. */
export function Rail() {
  return (
    <nav aria-label="Primary" className="w-[184px] flex-none overflow-y-auto border-r border-reco-line bg-reco-panel py-2.5">
      {NAV.map((group) => (
        <div key={group.label}>
          <div className="px-4 pt-3 pb-1 font-mono text-[9.5px] tracking-[0.15em] text-reco-t5">{group.label}</div>
          {group.items.map((item) => (
            <NavLink
              key={item.route}
              to={`/${item.route}`}
              className={({ isActive }) =>
                `mx-2 my-0.5 flex items-center justify-between rounded-md px-2.5 py-1.5 text-[12.5px] ${
                  isActive ? "bg-reco-row font-semibold text-reco-t1" : "font-normal text-reco-t3 hover:bg-reco-bg"
                }`
              }
            >
              <span className="overflow-hidden text-ellipsis whitespace-nowrap">{item.label}</span>
            </NavLink>
          ))}
        </div>
      ))}
    </nav>
  );
}
