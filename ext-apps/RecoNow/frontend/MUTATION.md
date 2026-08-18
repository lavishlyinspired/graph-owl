# Mutation testing

```
npm run mutate                      # the configured scope
npx stryker run --mutate src/lib/rows.ts   # one file, seconds
```

## What is mutated, and why the exclusions are not a dodge

`src/lib/*.ts` — the pure functions the screens call. Those functions exist
*because* they hold the decisions; extracting them from the components is what
makes the decisions reachable by a test at all.

Three exclusions, each for a stated reason rather than because it scored badly:

- **`screenConfigs.ts`** is 962 lines of per-screen config data — labels,
  copy, colours — lifted from the delivered mockup. Stryker generates ~2,780
  mutants there, almost all of them string-literal edits to UI copy. Killing
  one means asserting an exact copy string, which is a change-detector test:
  it fails on every wording change and catches no defect. Including this file
  drags the reported score to ~4% and that number measures nothing. **If a
  screen config grows a genuine decision, move the decision into its own
  module rather than widening this scope** — that is the same reason
  `visibleRows` was extracted.
- **`routes.ts`** is the route table, same shape of argument. It already has a
  structural test (`router.structural.test.ts`) asserting every route resolves
  to a real module, which is the property that actually matters.
- **`api.ts`** is generated-shaped DTO declarations with no branching.

`.tsx` components are not mutated. Mutating them measures React's rendering
more than our own logic. When a component grows logic worth mutating, the fix
is to extract it, not to widen the glob.

## Current state, stated honestly

**68.65% overall**, 19 August 2026.

| file | score | note |
|---|---|---|
| `format.ts` | **100%** | 11/11 |
| `rows.ts` | **100%** | 17/17 |
| `useClickOutside.ts` | 94% | 1 survivor |
| `nav.ts` | 67% | **38 survivors — a real gap, not yet worked** |
| `workspace.ts` | 27% total / 78% covered | 2 survivors, 17 mutants no test reaches |

The scope started at 54.59% and reached 68.65% by writing the tests
`format.ts` and `useClickOutside.ts` had never had — not by moving the
threshold, which was the other available way to make the number go up.

`break: 55` sits below today's score so CI fails on a regression without going
green on a lie. **Raise it as `nav.ts` and `workspace.ts` close** — a threshold
permanently far below the real score is a threshold nobody is defending.
