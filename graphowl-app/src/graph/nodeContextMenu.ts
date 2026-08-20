/** The content G6's `contextmenu` plugin hosts on right-click — Expand,
 *  Hide, and Open, built as the plain `HTMLElement` the plugin's `getContent`
 *  contract wants.
 *
 *  **`getContent`, not `getItems`.** The plugin's built-in `getItems` shortcut
 *  produces an unstyled white list (`rgba(255,255,255,0.96)`, hard-coded in
 *  its own CSS) that would sit wrong against this console's dark theme by
 *  default, and it only carries a flat `{name, value}` — no per-item
 *  disabled state, which "Expand" on an already-walked node needs. Building
 *  the content directly costs one more function and buys both back.
 *
 *  **Right-click, not hover.** A hover menu was tried first and reliably
 *  covered part of the graph the reader had not asked to see, appearing the
 *  moment the pointer crossed a node rather than on a deliberate gesture.
 *  `contextmenu` only opens on an explicit right-click, matching what a
 *  reader already expects from every other graph tool this console is
 *  modelled on. */

export interface NodeContextMenuLabels {
  readonly expand: string;
  readonly alreadyExpanded: string;
  readonly hide: string;
  readonly openEntity: string;
}

export interface NodeContextMenuOptions {
  readonly name: string;
  readonly alreadyExpanded: boolean;
  readonly labels: NodeContextMenuLabels;
  readonly onExpand: () => void;
  readonly onHide: () => void;
  readonly onOpen: () => void;
}

/** Colours read from the same CSS custom properties the rest of the console
 *  themes with, so this plain-DOM menu tracks light/dark without a second
 *  palette to keep in sync — `theme.css` sets these on `:root`, and this
 *  content mounts into the document, not into React, but `:root` still
 *  reaches it. */
const MENU_STYLE = `
  min-width: 176px;
  border: 1px solid var(--gowl-line);
  border-radius: 6px;
  background: var(--gowl-panel);
  overflow: hidden;
  box-shadow: 0 6px 16px rgba(0, 0, 0, 0.28);
  font-family: inherit;
`;

const TITLE_STYLE = `
  padding: 6px 10px;
  border-bottom: 1px solid var(--gowl-line);
  font-family: 'IBM Plex Mono', monospace;
  font-size: 9.5px;
  color: var(--gowl-t6);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
`;

const ACTION_STYLE = `
  display: block;
  width: 100%;
  padding: 6px 10px;
  border: none;
  background: transparent;
  text-align: left;
  font-size: 11.5px;
  color: var(--gowl-t3);
  cursor: pointer;
`;

const DIVIDER_STYLE = `
  height: 1px;
  margin: 4px 0;
  background: var(--gowl-line);
`;

function actionButton(
  action: "expand" | "hide" | "open",
  label: string,
  onClick: () => void,
): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.dataset["action"] = action;
  button.textContent = label;
  button.setAttribute("style", ACTION_STYLE);
  button.addEventListener("mouseenter", () => {
    button.style.background = "var(--gowl-row)";
    button.style.color = "var(--gowl-t1)";
  });
  button.addEventListener("mouseleave", () => {
    button.style.background = "transparent";
    button.style.color = "var(--gowl-t3)";
  });
  // A plain DOM listener on an element the plugin owns, entirely outside
  // G6's own event dispatch and outside React's synthetic event system — no
  // ref-based indirection is needed here, because neither system is involved
  // in delivering this click.
  //
  // Guarded explicitly rather than trusting `disabled` alone: a browser
  // suppresses a click on a disabled button, but that is enforcement at the
  // dispatch layer, not a property this listener can rely on every host
  // honours identically — and a disabled "Expand" must not re-expand an
  // already-walked node no matter how the click arrived.
  button.addEventListener("click", () => {
    if (button.disabled) return;
    onClick();
  });
  return button;
}

export function createNodeContextMenu(options: NodeContextMenuOptions): HTMLElement {
  const menu = document.createElement("div");
  menu.setAttribute("style", MENU_STYLE);

  const title = document.createElement("div");
  title.setAttribute("style", TITLE_STYLE);
  title.textContent = options.name;
  menu.appendChild(title);

  const expand = actionButton(
    "expand",
    options.alreadyExpanded ? options.labels.alreadyExpanded : options.labels.expand,
    options.onExpand,
  );
  expand.disabled = options.alreadyExpanded;
  if (options.alreadyExpanded) {
    expand.style.color = "var(--gowl-t7)";
    expand.style.cursor = "default";
  }
  menu.appendChild(expand);

  menu.appendChild(actionButton("hide", options.labels.hide, options.onHide));

  const divider = document.createElement("div");
  divider.setAttribute("style", DIVIDER_STYLE);
  menu.appendChild(divider);

  menu.appendChild(actionButton("open", options.labels.openEntity, options.onOpen));

  return menu;
}
