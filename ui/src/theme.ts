import type { ThemeConfig } from "antd";
import { theme } from "antd";

/** Ant Design (MIT) supplies the component look — buttons, panels, tables,
 *  form controls, density. What we set here is only the token layer: a blue
 *  primary in the category's conventional family, Inter, and a mono face for
 *  FQNs, where character-level differences carry meaning.
 *
 *  Deliberately not a transcription of another product's palette — see
 *  plans/00i-licensing.md. antd derives its whole ramp from one seed colour,
 *  which is what makes this legitimate rather than copied. */
const SEED = "#1570ef";

export const lightTheme: ThemeConfig = {
  algorithm: theme.defaultAlgorithm,
  token: {
    colorPrimary: SEED,
    colorLink: SEED,
    borderRadius: 6,
    fontFamily: `Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`,
    fontSize: 14,
    colorBgLayout: "#f8f9fc",
    colorBorderSecondary: "#eef1f6",
  },
  components: {
    Layout: {
      headerBg: "#ffffff",
      siderBg: "#ffffff",
      bodyBg: "#f8f9fc",
      headerHeight: 56,
    },
    Table: { headerBg: "#fafbfc", cellPaddingBlock: 10 },
  },
};

export const darkTheme: ThemeConfig = {
  algorithm: theme.darkAlgorithm,
  token: {
    ...lightTheme.token,
    colorBgLayout: "#0f1115",
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
