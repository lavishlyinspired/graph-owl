import type { ThemeConfig } from "antd";
import { theme } from "antd";

/** Ant Design (MIT) supplies the component layer. This is the token layer only,
 *  and from here it is coloured by the GraphOwl brand rather than by category
 *  convention.
 *
 *  The two disagree in one way that matters: the category's blue is a flat
 *  azure; ours is a navy-and-teal pair. Navy is the product's voice, teal is
 *  what indicates action. */
export const brand = {
  navy900: "#041D50", // "GRAPH" wordmark
  navy800: "#12356E",
  navy600: "#1F356A", // tagline
  blue600: "#2C64C8", // owl feathers
  teal500: "#03A7AD", // "OWL" wordmark
  teal600: "#028B92",
  cyan400: "#27C4D4", // accent lines and glow
  indigo400: "#6C74D8", // graph-circle gradient
  gold400: "#E6B14A", // beak and feet
} as const;

const FONT = `'Inter Variable', Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif`;

/** Light.
 *
 *  The logo's `#F8F7F3` is warm — right for a marketing page, slightly yellow
 *  behind dense data tables. Used at the page level with pure-white surfaces,
 *  which keeps the warmth as a hint rather than a cast. */
interface Palette {
  page: string;
  surface: string;
  raised: string;
  border: string;
  borderSoft: string;
  text: string;
  textMuted: string;
  textSubtle: string;
  selected: string;
  primary: string;
  primaryHover: string;
}

const LIGHT: Palette = {
  page: "#F8F7F3",
  surface: "#FFFFFF",
  raised: "#FFFFFF",
  border: "#E4E7F0",
  borderSoft: "#EEF0F6",
  text: brand.navy900,
  textMuted: "#3E5280",
  textSubtle: "#6B7BA3",
  selected: "#E6F7F8",
  primary: brand.teal500,
  primaryHover: brand.teal600,
};

/** Dark. Surfaces are navy-derived rather than neutral grey, so the same teal
 *  stays legible and the theme reads as the same product. Teal lightens,
 *  because `#03A7AD` on dark navy is too low-contrast to be a call to action. */
const DARK: Palette = {
  page: "#071129",
  surface: "#0E1E42",
  raised: "#142751",
  border: "#16305E",
  borderSoft: "#102046",
  text: "#E6ECF8",
  textMuted: "#9FB0D4",
  textSubtle: "#6C80AC",
  selected: "#0C3B47",
  primary: "#2BC4C9",
  primaryHover: "#4FD8DC",
};

function build(c: Palette, dark: boolean): ThemeConfig {
  return {
    algorithm: dark ? theme.darkAlgorithm : theme.defaultAlgorithm,
    token: {
      colorPrimary: c.primary,
      colorLink: c.primary,
      colorInfo: c.primary,
      colorBgLayout: c.page,
      colorBgContainer: c.surface,
      colorBgElevated: c.raised,
      colorBorder: c.border,
      colorBorderSecondary: c.borderSoft,
      colorText: c.text,
      colorTextSecondary: c.textMuted,
      colorTextDescription: c.textMuted,
      colorTextTertiary: c.textSubtle,
      colorTextHeading: c.text,
      // Gold reads as the mascot's beak, and on a data surface a warm yellow
      // means "warning" to everyone. Quarantined to exactly that.
      colorWarning: brand.gold400,
      borderRadius: 6,
      fontFamily: FONT,
      fontSize: 14,
      // The reference leans on 500/600 far more than antd's defaults, and that
      // weight is most of why it reads dense rather than thin.
      fontWeightStrong: 600,
      lineHeight: 1.5,
    },
    components: {
      Layout: {
        headerBg: c.surface,
        siderBg: c.surface,
        bodyBg: c.page,
        headerHeight: 56,
      },
      // Header takes the page tint so it reads as chrome, not a first row.
      Table: {
        headerBg: c.page,
        headerColor: c.textMuted,
        cellPaddingBlock: 10,
        borderColor: c.borderSoft,
        rowHoverBg: dark ? "#132650" : "#FBFAF7",
      },
      Card: { colorBorderSecondary: c.border, headerFontSize: 14 },
      Tree: { nodeSelectedBg: c.selected, nodeHoverBg: dark ? "#132650" : "#F3F2ED" },
      Menu: {
        itemSelectedBg: c.selected,
        itemSelectedColor: c.primary,
        itemHeight: 40,
      },
      Descriptions: { labelBg: c.page },
      Tag: { defaultBg: dark ? "#132650" : "#F1F0EB" },
      Timeline: { dotBg: c.surface },
    },
  };
}

export const lightTheme = build(LIGHT, false);
export const darkTheme = build(DARK, true);
export const palette = { light: LIGHT, dark: DARK };
