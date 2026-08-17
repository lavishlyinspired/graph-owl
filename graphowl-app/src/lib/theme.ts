export type ThemeMode = "dark" | "light";

const STORAGE_KEY = "graphowl-theme";

/** Dark is the product default (the mockup's own `:root` block carries the
 *  dark tokens with no `data-theme` needed) — light is the opt-in. */
export function resolveInitialTheme(storage: Pick<Storage, "getItem"> = window.localStorage): ThemeMode {
  const stored = storage.getItem(STORAGE_KEY);
  return stored === "light" ? "light" : "dark";
}

export function persistTheme(mode: ThemeMode, storage: Pick<Storage, "setItem"> = window.localStorage): void {
  storage.setItem(STORAGE_KEY, mode);
}

export function applyTheme(mode: ThemeMode, root: Pick<HTMLElement, "setAttribute"> = document.documentElement): void {
  root.setAttribute("data-theme", mode);
}

export function toggleTheme(mode: ThemeMode): ThemeMode {
  return mode === "dark" ? "light" : "dark";
}
