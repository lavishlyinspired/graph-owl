import type { ThemeConfig } from "antd";
import { theme } from "antd";

/** Ant Design (MIT) supplies the component layer. This is the token layer, and
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
} as const;

const FONT = `'Inter Variable', Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif`;

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
  primaryHover: primary.hover,
  success: "#22C55E",
  warning: "#FBBF24",
  error: "#EF4444",
  rowHover: "#1C3658",
  shadowSmall: "0 4px 10px rgba(0,0,0,.25)",
  shadowMedium: "0 12px 30px rgba(0,0,0,.35)",
  shadowLarge: "0 25px 70px rgba(0,0,0,.45)",
};

function build(c: Palette, dark: boolean): ThemeConfig {
  return {
    algorithm: dark ? theme.darkAlgorithm : theme.defaultAlgorithm,
    token: {
      colorPrimary: c.primary,
      colorLink: c.primary,
      colorBgLayout: c.page,
      colorBgContainer: c.surface,
      colorBgElevated: c.raised,
      colorFillSecondary: c.fill,
      colorFillQuaternary: c.fillSubtle,
      colorBorder: c.border,
      colorBorderSecondary: c.borderSoft,
      colorText: c.text,
      colorTextSecondary: c.textMuted,
      colorTextDescription: c.textMuted,
      colorTextTertiary: c.textSubtle,
      colorTextQuaternary: c.textDisabled,
      colorTextHeading: c.text,
      // Semantic colours are theme-aware. Status is the one thing a reader
      // must never have to squint at, and the same green cannot serve a white
      // page and a navy one.
      colorSuccess: c.success,
      colorWarning: c.warning,
      colorError: c.error,
      colorInfo: c.primary,
      borderRadius: radius.control,
      borderRadiusSM: radius.small,
      borderRadiusLG: radius.card,
      fontFamily: FONT,
      fontSize: 14,
      // 500/600 far more than antd's defaults; that weight is most of why the
      // result reads dense rather than thin.
      fontWeightStrong: 600,
      lineHeight: 1.5,
      boxShadowTertiary: c.shadowSmall,
      boxShadowSecondary: c.shadowMedium,
      boxShadow: c.shadowLarge,
    },
    components: {
      Layout: {
        headerBg: c.page,
        siderBg: c.sider,
        bodyBg: c.page,
        // Taller chrome: the header carries a logo, a search field and account
        // controls, and 56px forces all three to sit tight against each other.
        headerHeight: 72,
      },
      // The header takes the *section* tint, so a scrolled table still reads
      // as having a header rather than as starting mid-row.
      Table: {
        headerBg: c.surface,
        headerColor: c.textMuted,
        cellPaddingBlock: 12,
        borderColor: c.borderSoft,
        rowHoverBg: c.rowHover,
        borderRadiusLG: radius.card,
      },
      Card: {
        colorBorderSecondary: c.border,
        headerFontSize: 14,
        borderRadiusLG: radius.card,
      },
      Modal: { borderRadiusLG: radius.modal },
      Tree: { nodeSelectedBg: c.selected, nodeHoverBg: c.rowHover },
      Menu: {
        itemSelectedBg: c.selected,
        itemSelectedColor: c.primary,
        itemHeight: 40,
        itemBorderRadius: radius.control,
      },
      Descriptions: { labelBg: c.surface },
      Tag: { defaultBg: c.fill, borderRadiusSM: radius.small },
      Timeline: { dotBg: c.raised },
      Button: { borderRadius: radius.control, primaryShadow: "none" },
      Input: { borderRadius: radius.control },
      Statistic: { titleFontSize: 13 },
    },
  };
}

export const lightTheme = build(LIGHT, false);
export const darkTheme = build(DARK, true);
export const palette = { light: LIGHT, dark: DARK };
