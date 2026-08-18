/** Structural tests for the shadcn/ui-style primitives: wiring, not
 *  rendering. These wrap Radix UI (or are plain styled elements) and carry
 *  no behaviour of their own beyond forwarding props and merging classes —
 *  `00f-ui-architecture.md`'s 14 Aug 2026 revision replaces Ant Design with
 *  this component layer, and this is the first slice of it. Matches the
 *  regex-on-raw-source pattern already established for wiring components
 *  elsewhere in this codebase (e.g. `OntologyBuilder.structural.test.ts`). */

import { describe, expect, it } from "vitest";
import buttonSource from "./button.tsx?raw";
import inputSource from "./input.tsx?raw";
import labelSource from "./label.tsx?raw";
import textareaSource from "./textarea.tsx?raw";
import cardSource from "./card.tsx?raw";
import selectSource from "./select.tsx?raw";
import tabsSource from "./tabs.tsx?raw";
import dialogSource from "./dialog.tsx?raw";
import tooltipSource from "./tooltip.tsx?raw";
import popoverSource from "./popover.tsx?raw";

describe("Button", () => {
  it("derives its classes from cva variants, not a fixed class string", () => {
    expect(buttonSource).toMatch(/cva\(/);
    expect(buttonSource).toMatch(/variant:/);
    expect(buttonSource).toMatch(/size:/);
  });
  it("merges an incoming className with cn(), so a caller can override", () => {
    expect(buttonSource).toMatch(/cn\(/);
  });
  it("supports rendering as its child via asChild, Radix's own composition pattern", () => {
    expect(buttonSource).toMatch(/Slot/);
    expect(buttonSource).toMatch(/asChild/);
  });
});

describe("Input, Label, Textarea, Card", () => {
  it("Input forwards a ref and merges className", () => {
    expect(inputSource).toMatch(/forwardRef|React\.ComponentProps/);
    expect(inputSource).toMatch(/cn\(/);
  });
  it("Label wraps Radix's own accessible label primitive", () => {
    expect(labelSource).toMatch(/@radix-ui\/react-label/);
  });
  it("Textarea forwards a ref and merges className", () => {
    expect(textareaSource).toMatch(/cn\(/);
  });
  it("Card exposes a header/content/footer composition, not one monolithic box", () => {
    expect(cardSource).toMatch(/CardHeader/);
    expect(cardSource).toMatch(/CardContent/);
  });
});

describe("Select, Tabs, Dialog, Tooltip, Popover — Radix-backed", () => {
  it("Select wraps @radix-ui/react-select", () => {
    expect(selectSource).toMatch(/@radix-ui\/react-select/);
  });
  it("Tabs wraps @radix-ui/react-tabs", () => {
    expect(tabsSource).toMatch(/@radix-ui\/react-tabs/);
  });
  it("Dialog wraps @radix-ui/react-dialog", () => {
    expect(dialogSource).toMatch(/@radix-ui\/react-dialog/);
  });
  it("Tooltip wraps @radix-ui/react-tooltip", () => {
    expect(tooltipSource).toMatch(/@radix-ui\/react-tooltip/);
  });
  it("Popover wraps @radix-ui/react-popover", () => {
    expect(popoverSource).toMatch(/@radix-ui\/react-popover/);
  });
});
