# `_archived/`

Code and files removed from the live tree after a verified dead-code audit
(`plans/119-architecture-audit.md` §6) — evidence checked against imports,
registration/config/CLI/discovery, tests, CI, and docs, not just "grep found
no import." Nothing here was moved on a hunch.

Git history is the real archive — every item below can be recovered with
`git log --all --oneline -- <original path>` even without this directory.
What lives here is the *readable* record: what a thing was, why it existed,
why it was judged dead, and where to find it if that judgement turns out to
be wrong.

## Contents

- `demo-copy.sh` (originally `scripts/demo copy.sh`) — a stray, older,
  feature-incomplete duplicate of `scripts/demo.sh` (missing `--gst`, OIDC
  auto-detection, agent-service startup). Zero references anywhere.
- `rebuild_usage_rollups.md` — a Rust capability (trait method + two
  implementations + a facade wrapper) removed from four live files, not a
  standalone file. See that document for the full code and removal record.
- `samples-gst/` (originally `samples/gst/purchase-register-2026-07.json`
  and `gstr2b-2026-07.json`) — planted so `scripts/demo.sh --gst` had a
  register and a live-shaped GSTR-2B response to load (commit `5747e10`,
  11 August 2026: "pack installs, six rules run, nine findings open").
  Superseded when `demo.sh --gst` moved to loading `packs/gst` directly,
  which carries its own `fixtures/` (including `gstr2b-api-response.json`,
  the same live-GSP-shape role this pair played). Zero references anywhere
  by 16 August 2026 — `demo.sh` itself confirms the current fixture
  source ("the register and GSTR-2B come from packs/gst/fixtures/").

`examples/adapter-csv/` was also removed (an empty, untracked directory —
`scripts/verify-examples.sh`'s "adapter-csv" phase never actually read from
it). Nothing to archive there: an empty directory has no content to
preserve, and it was never tracked by git in the first place.
