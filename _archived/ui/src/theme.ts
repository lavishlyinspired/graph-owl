
/** This is the token layer, and
 *  it is the single source of truth for colour, radius and elevation — a hex
 *  literal in a component is a value nothing can theme.
 *
 *  Teal indicates action; navy is the product's voice. That pairing is the one
 *  place this deliberately departs from the flat azure the category defaults to.
 */
export const brand = {
  // Slate-navy rather than the old ink blue: it is the heading colour on every
  // surface, and a saturated navy at 15px reads as coloured text rather than
  // as black.
  navy900: "#0F172A",
  navy800: "#12356E",
  navy600: "#1F356A",
  blue600: "#2C64C8", // owl feathers
  teal500: "#14C3CF", // primary — actions, links, highlights
  teal600: "#0FAAB5", // primary hover
  cyan400: "#61DCE5", // secondary accents, chart series
  indigo400: "#6C74D8", // graph-circle gradient
  gold400: "#E6B14A", // beak and feet
} as const;

/** Brand colours that do not move between themes. Anything reading from here
 *  is asserting "this is GraphOwl", not "this is the current surface". */
export const primary = {
  base: "#14C3CF",
  hover: "#0FAAB5",
  light: "#61DCE5",
  soft: "#D9F9FB",
  // `base` fails WCAG AA text contrast (2.16:1 on white, 2.15:1 as white
  // text on it as a button fill — both need 4.5:1) — found by the Epic 39
  // Slice F first-run axe check flagging a real "Catalogue a source"
  // button, not a hypothetical. `action` is the same hue, darkened until it
  // passes: 5.98:1 white-on-it, 5.98:1 on white as text. `base`/`hover`
  // stay as the icon, border, and chart-series colour — contrast rules
  // bind text and solid UI fills, not every decorative pixel.
  action: "#0B6E77",
} as const;

/** Radii by role, not by number.
 *
 *  A single global radius makes a 4px tag and a 400px panel share a curve, and
 *  at that range the same value reads as two different intentions. Scaling with
 *  the element is what keeps the corner looking deliberate at both sizes. */
export const radius = {
  small: 8, // tags, badges
  control: 10, // buttons, inputs
  card: 16,
  modal: 20,
  panel: 24,
} as const;

interface Palette {
  page: string;
  surface: string;
  raised: string;
  sider: string;
  fill: string;
  fillSubtle: string;
  border: string;
  borderSoft: string;
  text: string;
  textMuted: string;
  textSubtle: string;
  textDisabled: string;
  selected: string;
  primary: string;
  /** Text/icon colour when the surface underneath is `selected` — kept apart
   *  from `primary` because they are not interchangeable: `primary` is tuned
   *  to read on `page`, `selected` is a light tint that a teal this light
   *  fails against. Found by the Epic 39 Slice F first-run axe check, which
   *  flagged the "Overview" selected menu item at a measured 1.94:1 against
   *  the WCAG AA 4.5:1 minimum for text — not a hypothetical, an actual
   *  failing contrast in the shipped theme. */
  selectedText: string;
  /** A solid fill meant to carry white text (a primary button) — distinct
   *  from `selectedText` because the two roles want opposite answers on
   *  dark: dark's `selected` tint is itself dark, so `primary.base` reads
   *  fine as text on it, but a *button* fill of `primary.base` still fails
   *  white text (2.15:1) regardless of theme, because the button's own
   *  background never changes with the page around it. */
  actionBg: string;
  primaryHover: string;
  success: string;
  warning: string;
  error: string;
  rowHover: string;
  shadowSmall: string;
  shadowMedium: string;
  shadowLarge: string;
}

/** Light. Page is pure white and *sections* are tinted — the inverse of the
 *  previous arrangement. A tinted page with white cards makes every card an
 *  object floating on a coloured field; a white page with tinted sections lets
 *  grouping be expressed by the tint instead of by a border on everything. */
const LIGHT: Palette = {
  page: "#FFFFFF",
  surface: "#F8FAFC",
  raised: "#FFFFFF",
  sider: "#FFFFFF",
  fill: "#F1F5F9",
  fillSubtle: "#F8FAFC",
  border: "#E5E7EB",
  borderSoft: "#EEF2F7",
  text: "#0F172A",
  textMuted: "#334155",
  textSubtle: "#64748B",
  textDisabled: "#94A3B8",
  selected: primary.soft,
  primary: primary.base,
  // Measured: primary.action on primary.soft (#D9F9FB) is 5.38:1, on white
  // 5.98:1 — both clear WCAG AA. primary.base itself is 1.94:1 on
  // primary.soft and 2.16:1 on white — it fails as text in either of its
  // two most common placements, which is why this is a distinct token
  // rather than a reuse.
  selectedText: primary.action,
  actionBg: primary.action,
  primaryHover: primary.hover,
  success: "#16A34A",
  warning: "#F59E0B",
  error: "#DC2626",
  rowHover: "#F1F5F9",
  shadowSmall: "0 2px 6px rgba(15,23,42,.05)",
  shadowMedium: "0 8px 24px rgba(15,23,42,.08)",
  shadowLarge: "0 20px 60px rgba(15,23,42,.12)",
};

/** Dark. Surfaces are navy-derived rather than neutral grey, so the same teal
 *  stays legible and the theme reads as the same product rather than as a
 *  greyscale inversion of it.
 *
 *  Semantic colours lighten here. `#16A34A` on `#0E1B2A` is legible but reads
 *  as muddy rather than as "good"; the darker surface needs a brighter signal
 *  to carry the same meaning. */
const DARK: Palette = {
  page: "#0E1B2A",
  surface: "#12233A",
  raised: "#152A45",
  sider: "#0B1624",
  fill: "#1D3557",
  fillSubtle: "#12233A",
  border: "#2B4566",
  // A translucent divider rather than a fixed navy: it sits correctly on both
  // the sider and the raised card, which are four steps apart in lightness.
  borderSoft: "rgba(255,255,255,0.06)",
  text: "#FFFFFF",
  textMuted: "#D1D5DB",
  textSubtle: "#94A3B8",
  textDisabled: "#64748B",
  selected: "#1D3557",
  primary: primary.base,
  // Dark's `selected` is a dark navy, not a light tint, so primary.base
  // already clears AA against it (5.73:1) — no separate token needed here.
  selectedText: primary.base,
  // A button's own fill does not change with the surrounding page, so this
  // fails white text at 2.15:1 in dark mode exactly as it does in light —
  // `primary.action` is theme-invariant for the same reason.
  actionBg: primary.action,
  primaryHover: primary.hover,
  success: "#22C55E",
  warning: "#FBBF24",
  error: "#EF4444",
  rowHover: "#1C3658",
  shadowSmall: "0 4px 10px rgba(0,0,0,.25)",
  shadowMedium: "0 12px 30px rgba(0,0,0,.35)",
  shadowLarge: "0 25px 70px rgba(0,0,0,.45)",
};

export const palette = { light: LIGHT, dark: DARK };

/** `--gowl-*` custom properties for the shadcn/Tailwind primitives
 *  (`components/ui/`) introduced in `00f-ui-architecture.md`'s 14 Aug 2026
 *  revision. `theme.ts` stays the single source of truth for colour and
 *  radius — a shadcn component reads these vars via `var(--gowl-*)` in a
 *  Tailwind arbitrary-value class rather than antd's `ConfigProvider`
 *  theme object, which only Ant Design components (still present until the
 *  console-wide swap finishes) can read. `AppShell` applies this to
 *  `document.documentElement` whenever `dark` toggles. */
export function cssVariables(colors: Palette): Record<string, string> {
  return {
    "--gowl-page": colors.page,
    "--gowl-surface": colors.surface,
    "--gowl-raised": colors.raised,
    "--gowl-sider": colors.sider,
    "--gowl-fill": colors.fill,
    "--gowl-fill-subtle": colors.fillSubtle,
    "--gowl-border": colors.border,
    "--gowl-border-soft": colors.borderSoft,
    "--gowl-text": colors.text,
    "--gowl-text-muted": colors.textMuted,
    "--gowl-text-subtle": colors.textSubtle,
    "--gowl-text-disabled": colors.textDisabled,
    "--gowl-selected": colors.selected,
    "--gowl-primary": colors.primary,
    "--gowl-selected-text": colors.selectedText,
    "--gowl-action-bg": colors.actionBg,
    "--gowl-primary-hover": colors.primaryHover,
    "--gowl-success": colors.success,
    "--gowl-warning": colors.warning,
    "--gowl-error": colors.error,
    "--gowl-row-hover": colors.rowHover,
    "--gowl-shadow-small": colors.shadowSmall,
    "--gowl-shadow-medium": colors.shadowMedium,
    "--gowl-shadow-large": colors.shadowLarge,
    "--gowl-radius-small": `${radius.small}px`,
    "--gowl-radius-control": `${radius.control}px`,
    "--gowl-radius-card": `${radius.card}px`,
    "--gowl-radius-modal": `${radius.modal}px`,
    "--gowl-radius-panel": `${radius.panel}px`,
  };
}
