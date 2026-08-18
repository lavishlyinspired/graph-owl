import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import GenericScreen from "./GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

/** The screen configs carry the delivered mockup's own illustrative numbers.
 *  They exist to describe layout — column widths, chart shapes, row density —
 *  not to stand in for data. A screen that renders them when it has no data
 *  is not "degraded", it is stating figures for a client's tax position that
 *  nobody computed. These tests pin the line: layout may come from the
 *  config, quantities may not. */

const renderScreen = (ui: React.ReactElement) =>
  render(<MemoryRouter>{ui}</MemoryRouter>);

describe("GenericScreen never presents mockup figures as data", () => {
  it("renders no KPI figures when the route supplies none", () => {
    // `suppliers` config carries ACTIVE SUPPLIERS 1,482 / WITH EXCEPTIONS 214.
    renderScreen(<GenericScreen config={screenConfig("suppliers")} liveRows={[]} />);

    expect(screen.queryByText("1,482")).not.toBeInTheDocument();
    expect(screen.queryByText("214")).not.toBeInTheDocument();
    expect(screen.queryByText("ACTIVE SUPPLIERS")).not.toBeInTheDocument();
  });

  it("renders the real KPI figures when the route supplies them", () => {
    renderScreen(
      <GenericScreen
        config={screenConfig("suppliers")}
        liveRows={[]}
        liveKpis={[{ label: "SUPPLIERS", value: "3", sub: "this period", color: "#1c1b18" }]}
      />,
    );

    expect(screen.getByText("3")).toBeInTheDocument();
    expect(screen.getByText("SUPPLIERS")).toBeInTheDocument();
    expect(screen.queryByText("1,482")).not.toBeInTheDocument();
  });

  it("shows an empty state rather than mockup rows when there is no data", () => {
    renderScreen(<GenericScreen config={screenConfig("suppliers")} liveRows={[]} />);

    expect(screen.queryByText(/XYZ Pvt Ltd/)).not.toBeInTheDocument();
    expect(screen.queryByText("₹4.82 L")).not.toBeInTheDocument();
    expect(screen.getByTestId("generic-empty")).toBeInTheDocument();
  });

  it("shows mockup rows for no screen at all, even one that passes nothing", () => {
    // An unwired route renders <GenericScreen config={...} /> with no liveRows.
    // That must not become a screen full of invented suppliers.
    renderScreen(<GenericScreen config={screenConfig("suppliers")} />);

    expect(screen.queryByText(/XYZ Pvt Ltd/)).not.toBeInTheDocument();
    expect(screen.queryByText(/ABC Suppliers/)).not.toBeInTheDocument();
  });

  it("renders no chart built from mockup values", () => {
    const viz = screenConfig("suppliers").viz;
    const firstLabel = "items" in viz && viz.items[0] ? viz.items[0].label : null;

    renderScreen(<GenericScreen config={screenConfig("suppliers")} liveRows={[]} />);

    expect(screen.queryByTestId("generic-viz")).not.toBeInTheDocument();
    if (firstLabel) expect(screen.queryByText(firstLabel)).not.toBeInTheDocument();
  });

  it("renders no assistant suggestion that quotes figures nobody computed", () => {
    // "₹8.2 L sits inside the s.16(4) window with 105 days left." — a figure
    // with no query behind it, under a heading that says "built from graph
    // facts". Asserted against the config's own text so this cannot pass by
    // looking for a string that lives on some other screen.
    const config = screenConfig("obligations");
    expect(config.copilot.text).toMatch(/₹/); // the fixture still carries it

    renderScreen(<GenericScreen config={config} liveRows={[]} />);

    expect(screen.queryByText(config.copilot.text)).not.toBeInTheDocument();
    expect(screen.queryByText(config.copilot.action)).not.toBeInTheDocument();
  });

  it("renders an assistant suggestion the caller supplies", () => {
    renderScreen(
      <GenericScreen
        config={screenConfig("obligations")}
        liveRows={[]}
        liveCopilot={{ text: "3 obligations fall due this week.", action: "Review them" }}
      />,
    );

    expect(screen.getByText("3 obligations fall due this week.")).toBeInTheDocument();
  });

  it("renders navigation link names without their invented counts", () => {
    renderScreen(<GenericScreen config={screenConfig("itc")} liveRows={[]} />);

    // The destination is real navigation and stays; the count beside it was invented.
    expect(screen.queryByText("24 pending")).not.toBeInTheDocument();
    expect(screen.queryByText("38 flagged")).not.toBeInTheDocument();
    expect(screen.queryByText("₹3.42 Cr")).not.toBeInTheDocument();
  });

  it("keeps explanatory copy that states no quantities", () => {
    // Guards the opposite error: `graphNote` explains how the product behaves
    // and asserts no figures. Stripping it would lose real product copy.
    const note = screenConfig("reset").graphNote;
    renderScreen(<GenericScreen config={screenConfig("reset")} liveRows={[]} />);

    expect(screen.getByText(note, { exact: false })).toBeInTheDocument();
  });
});
