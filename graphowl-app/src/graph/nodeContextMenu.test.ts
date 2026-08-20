import { describe, expect, it, vi } from "vitest";
import { createNodeContextMenu } from "./nodeContextMenu";

/** The content G6's own `contextmenu` plugin hosts on right-click — see
 *  `GraphCanvas.tsx`'s doc comment for why a right-click menu replaced the
 *  earlier hover one. Built and tested as plain DOM, independent of G6 and
 *  of React: the plugin's `getContent` wants exactly an `HTMLElement` back,
 *  and testing it as one means a click dispatched here is the same click
 *  the plugin's own event flow will dispatch. */
describe("the node context menu", () => {
  function menu(overrides?: {
    readonly alreadyExpanded?: boolean;
    readonly onExpand?: () => void;
    readonly onHide?: () => void;
    readonly onOpen?: () => void;
  }) {
    const onExpand = overrides?.onExpand ?? vi.fn();
    const onHide = overrides?.onHide ?? vi.fn();
    const onOpen = overrides?.onOpen ?? vi.fn();
    const el = createNodeContextMenu({
      name: "Patel Chemicals & Co",
      alreadyExpanded: overrides?.alreadyExpanded ?? false,
      labels: {
        expand: "Expand connections",
        alreadyExpanded: "Already expanded",
        hide: "Hide this node",
        openEntity: "Open entity",
      },
      onExpand,
      onHide,
      onOpen,
    });
    return { el, onExpand, onHide, onOpen };
  }

  it("titles the menu with the node's own name", () => {
    const { el } = menu();
    expect(el.textContent).toContain("Patel Chemicals & Co");
  });

  it("calls onExpand when the expand action is clicked", () => {
    const { el, onExpand } = menu();
    const button = el.querySelector<HTMLButtonElement>('[data-action="expand"]');
    button?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(onExpand).toHaveBeenCalledOnce();
  });

  it("calls onHide when the hide action is clicked", () => {
    const { el, onHide } = menu();
    const button = el.querySelector<HTMLButtonElement>('[data-action="hide"]');
    button?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(onHide).toHaveBeenCalledOnce();
  });

  it("calls onOpen when the open action is clicked", () => {
    const { el, onOpen } = menu();
    const button = el.querySelector<HTMLButtonElement>('[data-action="open"]');
    button?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(onOpen).toHaveBeenCalledOnce();
  });

  /** A node already walked has nothing left to expand — offering the action
   *  anyway invites a click that silently does nothing, so it is disabled
   *  and relabelled rather than removed (removing it would make the menu's
   *  height and the position of the actions below it jump depending on the
   *  node). */
  it("disables and relabels expand for a node already walked", () => {
    const { el, onExpand } = menu({ alreadyExpanded: true });
    const button = el.querySelector<HTMLButtonElement>('[data-action="expand"]');
    expect(button?.disabled).toBe(true);
    expect(button?.textContent).toBe("Already expanded");
    button?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(onExpand).not.toHaveBeenCalled();
  });

  it("keeps hide clickable even when the node is already expanded", () => {
    const { el, onHide } = menu({ alreadyExpanded: true });
    const button = el.querySelector<HTMLButtonElement>('[data-action="hide"]');
    button?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(onHide).toHaveBeenCalledOnce();
  });

  /** One label, for either id shape — `entity.tsx` now branches internally
   *  (a catalog asset through the asset endpoints, a graph-only subject
   *  through `/graph/context` and `/findings`) the way Explore's own fetch
   *  already did, so the menu no longer needs to guess which kind of entity
   *  page it is opening. */
  it("labels the open action", () => {
    const { el } = menu();
    const button = el.querySelector<HTMLButtonElement>('[data-action="open"]');
    expect(button?.textContent).toBe("Open entity");
  });
});
