import { describe, expect, it, vi } from "vitest";
import { renderHook } from "@testing-library/react";
import { useClickOutside } from "./useClickOutside";

const mount = (onOutside: () => void, active: boolean) => {
  const element = document.createElement("div");
  const inside = document.createElement("button");
  element.append(inside);
  document.body.append(element);

  const ref = { current: element };
  const view = renderHook(() => useClickOutside(ref, onOutside, active));

  return { element, inside, view, cleanup: () => element.remove() };
};

const mousedownOn = (target: Element) =>
  target.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));

describe("useClickOutside", () => {
  it("fires when the click lands outside the element", () => {
    const onOutside = vi.fn();
    const { cleanup } = mount(onOutside, true);

    mousedownOn(document.body);

    expect(onOutside).toHaveBeenCalledTimes(1);
    cleanup();
  });

  it("does not fire when the click lands inside", () => {
    // The bug this hook was written for was a dropdown that closed on its own
    // trigger; closing on every click including its own contents would be the
    // same defect wearing the other sign.
    const onOutside = vi.fn();
    const { inside, cleanup } = mount(onOutside, true);

    mousedownOn(inside);

    expect(onOutside).not.toHaveBeenCalled();
    cleanup();
  });

  it("does not fire on the element itself", () => {
    const onOutside = vi.fn();
    const { element, cleanup } = mount(onOutside, true);

    mousedownOn(element);

    expect(onOutside).not.toHaveBeenCalled();
    cleanup();
  });

  it("listens for nothing while inactive", () => {
    const onOutside = vi.fn();
    const { cleanup } = mount(onOutside, false);

    mousedownOn(document.body);

    expect(onOutside).not.toHaveBeenCalled();
    cleanup();
  });

  it("stops listening once unmounted", () => {
    // A dropdown that closes and leaves its listener behind keeps calling a
    // handler for a thing no longer on screen.
    const onOutside = vi.fn();
    const { view, cleanup } = mount(onOutside, true);

    view.unmount();
    mousedownOn(document.body);

    expect(onOutside).not.toHaveBeenCalled();
    cleanup();
  });

  it("does nothing when the ref holds no element yet", () => {
    const onOutside = vi.fn();
    const ref = { current: null };
    renderHook(() => useClickOutside(ref, onOutside, true));

    mousedownOn(document.body);

    expect(onOutside).not.toHaveBeenCalled();
  });
});
