/** G6 draws to a `<canvas>`, which cannot resolve a CSS custom property —
 *  `fill: "var(--gowl-accent)"` paints nothing. These are literal hex values
 *  mirrored from `theme.css`'s own `:root`/`[data-theme="light"]` blocks
 *  (themselves lifted verbatim from the delivered mockup), not a second
 *  source of truth invented for the canvas — keep the two in sync by eye
 *  whenever `theme.css`'s dark or light block changes. */

import type { StyleColors } from "./graphModel";

export const GRAPH_COLORS: Record<"dark" | "light", StyleColors> = {
  dark: {
    text: "#e6e9ee", // --gowl-t1
    primary: "#5cc7d8", // --gowl-accent
    border: "#232a33", // --gowl-line-2
    raised: "#12161b", // --gowl-panel-2
  },
  light: {
    text: "#1b1a17", // --gowl-t1
    primary: "#0f7f92", // --gowl-accent
    border: "#dcd8d0", // --gowl-line-2
    raised: "#ffffff", // --gowl-panel-2
  },
};
