# Design

Source of truth for tokens: `ui/src/theme.ts`. This file is a map into it, not a duplicate — read `theme.ts` before inventing a new value.

## Color

Light-page, tinted-section model: page is pure white/near-black-navy; cards and panels are a step lighter/darker than the page, expressing grouping via fill rather than borders everywhere.

| Role | Light | Dark |
|---|---|---|
| Page | `#FFFFFF` | `#0E1B2A` |
| Surface / Sider | `#F8FAFC` / `#FFFFFF` | `#12233A` / `#0B1624` |
| Raised (card) | `#FFFFFF` | `#152A45` |
| Fill / Fill subtle | `#F1F5F9` / `#F8FAFC` | `#1D3557` / `#12233A` |
| Border / Border soft | `#E5E7EB` / `#EEF2F7` | `#2B4566` / `rgba(255,255,255,.06)` |
| Text / muted / subtle / disabled | `#0F172A` / `#334155` / `#64748B` / `#94A3B8` | `#FFFFFF` / `#D1D5DB` / `#94A3B8` / `#64748B` |
| Primary (teal) | `#14C3CF`, hover `#0FAAB5` | same |
| Action fill (WCAG-safe teal) | `#0B6E77` (theme-invariant — a button fill doesn't change with the page) |
| Success / Warning / Error | `#16A34A` / `#F59E0B` / `#DC2626` | `#22C55E` / `#FBBF24` / `#EF4444` (brighter — darker surface needs a stronger signal) |

Brand: navy (`#0F172A` heading ink) + teal (`#14C3CF` primary) is the one deliberate departure from this category's default flat-azure look. Never introduce a third accent hue without a stated reason.

**Contrast is load-bearing, not decorative** — `primary.base` (`#14C3CF`) fails WCAG AA as text (2.16:1 on white) and is icon/border/chart-series only; `selectedText`/`actionBg` use the darkened `#0B6E77` for any text-on-teal or teal-as-button-fill case. Check new components against this split before shipping.

## Typography

- Sans: **Inter Variable** (`@fontsource-variable/inter`) — imported but, before this pass, never applied to `body`; the app was silently rendering the OS system-font stack. Fixed in `base.css`.
- Mono: **JetBrains Mono** for FQNs, code, identifiers (`code, .fqn` selector) — deliberate: proportional type hides the character-level differences that make an FQN meaningful.
- Display face (`@fontsource-variable/sora`) imported but not yet wired to a heading style — product register per PRODUCT.md calls for one family carrying everything; treat Sora as available for a rare, deliberate moment (e.g. the app wordmark), not page headings.
- Scale: tight ratio (product register, not brand) — 12/13/14/16/18/20/24px in practice across the console; don't introduce a fluid/clamp scale.

## Radius (by role, not by number)

`small: 8px` (tags/badges) · `control: 10px` (buttons/inputs) · `card: 16px` · `modal: 20px` · `panel: 24px`. A 4px tag and a 400px panel sharing one radius is the tell this scale exists to avoid.

## Shadow

Small/medium/large, tuned per theme (dark needs deeper/higher-opacity shadows to read against a dark surface): light `0 2px 6px rgba(15,23,42,.05)` → `0 20px 60px rgba(15,23,42,.12)`; dark `0 4px 10px rgba(0,0,0,.25)` → `0 25px 70px rgba(0,0,0,.45)`.

## Components

`ui/src/components/ui/*` — a shadcn-style layer over Radix UI primitives (migrated off Ant Design, see `plans/00f-ui-architecture.md`'s 14 Aug 2026 revision). Read the existing primitive before adding a new one; most antd-shaped call sites (`Card`, `Table`, `Select`, `Space`, `Tag`, `Statistic`, `Tree`, `Tabs`) already have a compat wrapper — extend it rather than reaching for a raw Radix primitive or a new dependency.

Known-fixed layout gotchas worth knowing before touching `components/ui/layout.tsx` or `space.tsx` again:
- `Layout` needs `flex-auto` on itself (matches real antd) or a nested `Layout` collapses to its own content height instead of filling its parent.
- `Space` (vertical) must not force `items-center` — only horizontal `Space` centers by default; vertical stretches.
