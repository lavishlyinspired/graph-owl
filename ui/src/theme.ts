import type { ThemeConfig } from "antd";
import { theme } from "antd";

/** Ant Design (MIT) supplies the component layer. This is the token layer only.
 *
 *  Values follow the conventions of this product category — a gray-blue page,
 *  white surfaces, cool borders, a familiar blue primary. Individual colour and
 *  size values are not protectable expression; what `plans/00i-licensing.md`
 *  prohibits is transcribing another product's stylesheets, components, or its
 *  palette as a system. antd derives its full ramp from the seed below. */
const PRIMARY = "#1570ef";

const FONT = `'Inter Variable', Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif`;

/** Light. Page is gray-blue rather than white or grey, which is what makes a
 *  white card read as raised without a shadow on every one. */
const LIGHT = {
  page: "#f8f9fc",
  surface: "#ffffff",
  raised: "#ffffff",
  border: "#eaecf5",
  borderSoft: "#f0f2f7",
  text: "#1c1f26",
  textMuted: "#5c6270",
  textSubtle: "#8c8c8c",
  selected: "#eff8ff",
};

/** Dark. Not an inversion — the surfaces are cool-neutral so the same blue
 *  primary stays legible against them, and borders lift rather than recede. */
const DARK = {
  page: "#0e1116",
  surface: "#161a21",
  raised: "#1b2029",
  border: "#2a303b",
  borderSoft: "#20262f",
  text: "#e9ecf2",
  textMuted: "#a3abb9",
  textSubtle: "#767e8c",
  selected: "#15243a",
};

function build(c: typeof LIGHT, dark: boolean): ThemeConfig {
  return {
    algorithm: dark ? theme.darkAlgorithm : theme.defaultAlgorithm,
    token: {
      colorPrimary: PRIMARY,
      colorLink: dark ? "#5aabff" : PRIMARY,
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
      borderRadius: 6,
      fontFamily: FONT,
      fontSize: 14,
      // The reference leans on 500/600 far more than antd's defaults do, and
      // that weight is most of why it reads dense and deliberate rather than
      // thin. Headings go to 600 explicitly.
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
        rowHoverBg: dark ? "#1b2029" : "#fafbfd",
      },
      Card: { colorBorderSecondary: c.border, headerFontSize: 14 },
      Tree: { nodeSelectedBg: c.selected, nodeHoverBg: dark ? "#1b2029" : "#f5f7fa" },
      Menu: {
        itemSelectedBg: c.selected,
        itemSelectedColor: PRIMARY,
        itemHeight: 40,
      },
      Descriptions: { labelBg: c.page },
      Tag: { defaultBg: dark ? "#20262f" : "#f4f6fa" },
    },
  };
}

export const lightTheme = build(LIGHT, false);
export const darkTheme = build(DARK, true);
export const palette = { light: LIGHT, dark: DARK };
