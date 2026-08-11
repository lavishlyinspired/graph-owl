# Plan: F1 — pack management admin tab

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026.
**Files**: `ui/src/features/packs/packSurfaces.ts` (`installedPacks`, new),
`ui/src/features/packs/packSurfaces.test.ts` (6 new tests),
`ui/src/features/packs/PackAdminPanel.tsx` (new),
`ui/src/App.tsx` (new "Packs" admin tab, `setSection` threaded into
`AdminPage`).

## The gap

`PackImportPanel.tsx` answers "how do I load data into a pack" — one
file-upload card per surface, plus a reconcile button, embedded inside
the Connectors page. It does not answer "which packs are installed" or
let an admin move between what a pack has (obligations, review) without
navigating there by hand. Named directly in the completion list: "a
dedicated pack-management admin tab (switching/browsing packs) —
`PackImportPanel.tsx` exists but only handles import, not this."

## What was built

**`installedPacks()`** (`packSurfaces.ts`) — deliberately not a reuse of
the existing `surfacesFor()`. `surfacesFor` filters to
`imports.length > 0`, which is correct for its own job (nothing to
render for a pack with no upload surface) but wrong for browsing: a pack
registered only through `graph-owl-load-pack`, with no upload surface of
its own, would be invisible to an admin trying to confirm it loaded at
all. `installedPacks` reads the same `GET /namespaces` `declaredBy:
"pack:<id>"` discovery mechanism but keeps every distinct pack id,
deduplicated across a pack that declares more than one namespace (`105c`'s
law namespace beside its main one), with a friendly label from the
registry when available and a title-cased fallback otherwise.

**`PackAdminPanel.tsx`** — a new "Packs" tab in `AdminPage`, listing every
installed pack (pack id, label, one representative namespace) with two
actions per row:

- **Reconcile now** — the same `api.reconcilePack` call
  `PackImportPanel`'s own `ReconcileButton` already makes, so triggering a
  pack's rules does not require finding the file-upload page first.
- **View obligations** — the "switching" half. Calls the already-exported-
  but-previously-uncalled `setObligationCalendarParams({ pack })` (its own
  doc comment: "kept for callers that want to change `?pack=`/`?windowDays=`
  without...") then `setSection("obligations")`, threaded down from
  `App.tsx` as a prop rather than owned by the panel — it has no reason to
  know how the console navigates between sections, only that it can ask.

**Found while building this**: the obligation calendar's own default is
`readParam("pack") ?? "gst"` — a hardcoded fallback that would silently
point a hospitality-only deployment at a pack that does not exist for
them. Not fixed here (out of this slice's scope — the fallback only
matters when nobody has picked a pack yet, and this panel makes picking
one a one-click action), but worth recording: `installedPacks()` is now
the generic way to ask "what should this default actually be" if that
fallback is ever revisited.

## Browser verification

Real end-to-end, not just unit tests: `./scripts/demo.sh --secure`
(self-signed HS256, no external identity provider), GST pack loaded via
`graph-owl-load-pack`, signed in through the console with the script's
own printed root token. Confirmed:

- Admin → Packs lists the real installed GST pack (id, label, namespace
  code and IRI) — not a fixture, the demo's own seeded catalogue.
- **Reconcile now** issues a real `POST /packs/gst/reconcile` (200,
  confirmed via the network panel), button returns to idle after.
- **View obligations** navigates to Obligations with `?section=
  obligations&pack=gst` in the URL and the real obligation calendar
  renders two real rows (`purchase-INV-2002` overdue,
  `purchase-INV-1006` upcoming) — proving the "switching" mechanism
  reaches real, pack-scoped data, not just that the URL changed.
- No console errors from the app itself (two `MaxListenersExceededWarning`
  entries present are from an unrelated browser extension, not this
  code).

## Test coverage

`installedPacks()`: 6 new tests — empty deployment, a pack with no
import surface (the property `surfacesFor` cannot provide), registry
label vs title-cased fallback, dedup across multiple namespaces from one
pack, and the same connector-boundary `packIdOf` already draws.
`packSurfaces.test.ts` 15 tests total (was 9). No dedicated `.test.tsx`
for `PackAdminPanel.tsx` itself — this project's own convention (zero
`.test.tsx` files exist anywhere in `ui/src`) is pure logic in `.ts` with
unit tests, components verified against a real running server in a real
browser, which is what the section above records.

## What this deliberately does not do

- **Does not fix the obligation calendar's hardcoded `"gst"` default.**
  Real, found while building this, out of scope for this slice — see
  above.
- **Does not add a pack-uninstall or pack-delete action.** Browsing and
  switching were the named gap; removal is real, separate product surface
  with its own data-retention questions (`00g`) this slice does not
  attempt.
- **Does not merge with `PackImportPanel`.** Deliberately two components:
  an admin confirming a pack loaded should not scroll past every upload
  surface it declares to find out, and a pack with no upload surface
  needs a `installedPacks`-shaped list `surfacesFor` cannot provide.
