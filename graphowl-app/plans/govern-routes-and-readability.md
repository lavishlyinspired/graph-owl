# Plan: Fix the GOVERN route collision (Validation/Contradictions/Resolution); a second, larger text-size pass

**Status**: Complete

## Goal

Two follow-ups reported directly against the app, not from a code review: (1) the entire GOVERN nav group appeared to not work at all, and (2) the earlier text-size increase (`studio-queries-tab.md`) wasn't enough — still unreadable at 100% browser zoom.

## Bug: `/validation`, `/contradictions`, `/resolution` unreachable by direct navigation

**Root cause**: `vite.config.ts`'s dev-only proxy forwards any request matching `^/validation(/|$|\?)`, `^/contradictions(/|$|\?)`, `^/resolution(/|$|\?)` straight to `graph-owl-server` on `:8080` — required, because the app's own API calls (`/validation/report`, `/validation/waivers`, `/resolution/queue`, `/contradictions/reviews`) live under those same prefixes. A **document-level** GET to the bare route (hard refresh, typed URL, bookmark — anything that isn't React Router's own client-side `<Link>`, which never touches the network) collides with that same proxy rule and gets forwarded to :8080 before Vite's SPA fallback ever sees it. The backend served back a stale build (`/static/index-CZwzPgxb.js`, itself a 404), so the browser showed a blank white page with an empty `#root`.

Confirmed via direct `curl`: `/validation`, `/contradictions`, `/resolution` all returned production-style HTML referencing a non-existent hashed JS bundle, while `/home`, `/explore`, `/studio`, `/governance`, `/drift-view` (already renamed once for this exact reason) returned correct Vite dev HTML. Client-side `<Link>` navigation from within the already-loaded SPA worked fine the whole time — the bug only bites on a real network round-trip, which is why it could sit unnoticed: nothing in the fast `vitest` loop does a real browser navigation, and the one Playwright spec that does (`tests/first-run.spec.ts`) is an E2E suite requiring a live server, not part of the routine gate, and had already drifted (`/lineage-view`, `/workbench` — both deleted earlier this session).

This is the exact same bug class already fixed once, documented in `routes.ts`'s own comment: `home`/`drift-view`/`mcp-tools` were renamed away from `overview`/`drift`/`mcp` for precisely this reason. `validation`/`contradictions`/`resolution` were simply never renamed when that fix landed.

**Fix**: renamed the console's own route slugs only (never the API paths, which are the versioned product surface) — `validation` → `validation-view`, `contradictions` → `contradictions-view`, `resolution` → `resolution-view`, matching `drift-view`'s existing precedent exactly. Nav labels unchanged ("Validation", "Contradictions", "Resolution" still shown in the sidebar).

- `src/routes/validation.tsx` → `validation-view.tsx`, `contradictions.tsx` → `contradictions-view.tsx`, `resolution.tsx` → `resolution-view.tsx` (renamed, not edited — `router.tsx` derives both the URL path and the lazy-loaded filename from the same `ROUTES` entry).
- `src/lib/routes.ts`: `ROUTES` updated; comment extended with this second occurrence of the collision.
- `src/lib/nav.ts`: the three `route:` fields updated to match.
- `src/lib/nav.test.ts`: `pageTitleForPath("/validation")` → `pageTitleForPath("/validation-view")`.
- `tests/first-run.spec.ts`: its hardcoded per-nav-group route list was already stale (`/lineage-view`, `/workbench` — both deleted earlier this session) — rebuilt to the current 7 nav groups: `/home`, `/explore`, `/studio`, `/validation-view`, `/sources`, `/analytics`, `/knowledge`.
- **Verified live**: direct hard-navigation (`curl`, and a real browser `goto`, not a client-side `<Link>` click) to all three renamed routes now returns real rendered content — `#root` went from `0` chars to 7,000+ on each, screenshotted (Resolution: real KPI row + empty-state table, sidebar active-state correct).
- 401/401 unit tests pass, `tsc` clean.

## Text size: second pass, +4px cumulative from the original design

The first pass (`studio-queries-tab.md`) added +1.5px per size tier. Reported as still unreadable at 100% zoom, so a second uniform pass added **+2.5px more on top of the already-bumped values** (same single-pass-regex-with-lookup method, for the same reason: the new value set overlaps the current value set, e.g. `13.5px`→`16px` while a separately-targeted `14px`→`16.5px`, so sequential `sed` would double-bump anything already rewritten to a value another rule still targets).

Cumulative effect from the *original* design (before either pass): every size is now **+4px**. The most common body-text tier (originally 12px, 327 combined occurrences across three neighboring sizes) now renders at 14.5–16px; the smallest micro-labels (originally 8–9.5px) now render at 12–13.5px instead of being effectively unreadable; headings (originally 18–21px) now render at 21–25px.

- 733 substitutions, 49 files, both passes — verified no collisions (per-bucket counts matched exactly before/after both times).
- [x] `tsc` clean, 401/401 tests pass.
- [x] Verified live on Home (KPI grid + activity feed), Explore's Entity panel (all 6 tabs, dense fact rows and side cards), and Studio (all 9 tabs in one row) — text is clearly legible at 100% zoom, no overflow, clipping, or tab-bar wrapping introduced by the larger sizes.

---
*Delete this file when the plan is complete.*
