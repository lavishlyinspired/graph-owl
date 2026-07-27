import type { ThemeConfig } from "antd";
import { theme } from "antd";

/** Ant Design (MIT) supplies the component layer — buttons, tables, forms,
 *  tree, panels. What is set here is the token layer only.
 *
 *  The values match the visual conventions of this product category: a
 *  near-white gray-blue page, white surfaces, a cool #eaecf5 border, and the
 *  familiar #1570ef primary. Individual colour values are not protectable
 *  expression; what plans/00i-licensing.md prohibits is transcribing another
 *  product's stylesheets, components, or its palette *as a system*. antd
 *  derives its whole ramp from the seed below. */
const PRIMARY = "#1570ef";

/** Page background. Gray-blue rather than pure white or pure grey — it is what
 *  makes white cards read as raised without needing a shadow on every one. */
const PAGE_BG = "#f8f9fc";
const SURFACE = "#ffffff";
const BORDER = "#eaecf5";
const TEXT = "#262626";
const TEXT_MUTED = "#757575";
const TEXT_SUBTLE = "#8c8c8c";

export const lightTheme: ThemeConfig = {
  algorithm: theme.defaultAlgorithm,
  token: {
    colorPrimary: PRIMARY,
    colorLink: PRIMARY,
    colorBgLayout: PAGE_BG,
    colorBgContainer: SURFACE,
    colorBorder: BORDER,
    colorBorderSecondary: BORDER,
    colorText: TEXT,
    colorTextSecondary: TEXT_MUTED,
    colorTextDescription: TEXT_MUTED,
    colorTextTertiary: TEXT_SUBTLE,
    borderRadius: 6,
    fontFamily: `Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`,
    fontSize: 14,
  },
  components: {
    Layout: {
      headerBg: SURFACE,
      siderBg: SURFACE,
      bodyBg: PAGE_BG,
      headerHeight: 56,
    },
    // The page tint again on table headers, so a header reads as chrome
    // rather than as a first row.
    Table: { headerBg: PAGE_BG, cellPaddingBlock: 10, borderColor: BORDER },
    Card: { colorBorderSecondary: BORDER },
    Tree: { nodeSelectedBg: "#eff8ff" },
    Menu: { itemSelectedBg: "#eff8ff" },
  },
};

export const darkTheme: ThemeConfig = {
  algorithm: theme.darkAlgorithm,
  token: {
    colorPrimary: PRIMARY,
    colorLink: "#53b1fd",
    colorBgLayout: "#0f1115",
    colorBgContainer: "#161920",
    colorBorder: "#262b36",
    colorBorderSecondary: "#1c202a",
    borderRadius: 6,
    fontFamily: lightTheme.token!.fontFamily,
    fontSize: 14,
  },
  components: {
    Layout: {
      headerBg: "#161920",
      siderBg: "#161920",
      bodyBg: "#0f1115",
      headerHeight: 56,
    },
    Table: { headerBg: "#1a1e27", cellPaddingBlock: 10 },
  },
};
