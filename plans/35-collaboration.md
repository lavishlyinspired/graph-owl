# Plan: Collaboration (Epic 35)
**Branch**: feat/collaboration
**Status**: Backend shipped (Slices A–F), console surface deferred to Epic 42
**Depends on**: Epic 11 (users), Epic 12 (real identity on posts)
**Crates**: `graph-owl-core` (Thread, Post, Proposal, Announcement, Reaction) · `graph-owl-storage-postgres` · `graph-owl-api` (attribution split) · `graph-owl-server`

## Goal

Let people ask about an asset where the asset lives, and propose corrections without needing write access.

## Why it matters more than it looks

Catalogs are abandoned when they become stale reference material nobody trusts. The systems that survive are the ones where the conversation about a dataset happens *on* the dataset — questions, answers, and corrections accumulating as context rather than evaporating into chat.

The change-proposal mechanism is the load-bearing part: it converts "this description is wrong" from a complaint into a reviewable contribution, and lets a steward accept a fix from someone without write permission.

## Resolved decisions

1. **Threads attach to an entity or to a specific field.** "This column's description is wrong" is a different conversation from "who owns this table". Field-level anchoring is what makes discussion actionable instead of a comment wall.
2. **Change proposals are distinct from discussions.** A proposal carries a concrete diff and an accept/reject outcome; accepting applies it with correct attribution.
3. **Accepting a proposal attributes the change to the proposer**, not the accepter, with the accepter recorded as approver. Otherwise contribution history is wrong and nobody contributes twice.
4. **No real-time transport.** Polling plus Epic 3's change events are sufficient. WebSockets would be a large operational addition for a workflow measured in hours.
5. **Announcements have a validity window** and disappear when it closes. A permanent banner is ignored within a week.
6. **Reactions, not threaded replies-to-replies.** One level of reply plus reactions covers the need; nested threads are a UI problem the API should not create.

## Acceptance criteria (feature level)

- [x] A user can start a thread on an entity or a field, and others can reply.
- [x] A thread can be resolved, and resolved threads are filterable.
- [x] A user can propose a field change; a steward can accept or reject it.
- [x] An accepted proposal applies the change attributed to the proposer.
- [x] An announcement displays on an entity for a validity window and then stops.
- [x] An activity feed shows discussions and Epic 3 change events together, in order.
- [x] Deleting an entity retains its threads for audit; hard delete removes them —
      proven at the schema level (`ON DELETE CASCADE`) since this project's `Storage`
      trait has no hard-delete for assets at all yet; see "Explicitly deferred" below.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Threads and replies

**Value**: The conversation has a home.
**Path**: `Thread { about, field?, message, created_by, resolved }` with `Post` replies.
**Acceptance criteria**:
- Start a thread on an entity; optionally anchored to a field or column FQN.
- Reply to a thread; replies are ordered and paginated.
- A thread anchored to a nonexistent field → `400`.
- `GET /tables/{id}/threads` paginated, filterable by resolved state.
- Author is the authenticated principal, never client-supplied.
- Editing a post is allowed by its author within a window and records that it was edited.
- Deleting a post tombstones it, preserving thread structure rather than leaving a hole.
**RED**: Test asserting a client-supplied author is ignored in favour of the principal — the trust boundary. Post-delete test asserting thread structure survives. Mutator watch: trusting a client-supplied author must fail the first.
**GREEN**: entities, anchoring, principal-derived authorship.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped, 6 August 2026.** The trust boundary is structural rather than checked at
runtime: `StartThreadRequest`/`ReplyRequest` have no `createdBy`/`author` field at all,
so there is nothing for a client to send that could set one — the same "PATCH
immutability via DTO shape" pattern Epic 2's `TableUpdate` already uses, proven at the
wire with a test that sends an extra `createdBy` field and asserts it is silently
dropped. **Scope cut, recorded rather than silently narrowed**: "a thread anchored to a
nonexistent field → `400`" is not implemented — `field` is validated non-empty, but
there is no per-entity-kind schema this project can check a column FQN against (a
`Table`'s columns are not their own addressable entities today), so a bad field name is
accepted rather than rejected. Revisit once column-level addressing exists.

### Slice B: Threads resolve

**Value**: Answered questions stop looking like open ones.
**Path**: resolve/reopen transitions recording who and when.
**Acceptance criteria**:
- Resolve records resolver and timestamp; reopen clears it.
- Resolving an already-resolved thread → `409`.
- Filter threads by `resolved=true|false`.
- The thread author or an entity owner may resolve; others → `403`.
- Resolution emits an event so the activity feed reflects it.
- An unresolved-thread count is available on entity read via field selection.
**RED**: Authorization test asserting an unrelated user cannot resolve. Mutator watch: an unconditional permit must fail it.
**GREEN**: transitions, authorization, counts.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped, 6 August 2026.** `unresolved_thread_count` exists on `Storage`/`Catalog` and
is HTTP-reachable, but only as its own read — **not** folded into a `Table`/`Asset`
read via field selection, the way the acceptance criterion asks. Epic 2's field
selection is a fixed, per-endpoint whitelist of columns already on the entity row; a
computed cross-table count is a different shape of extension than that mechanism was
built for. Recorded rather than silently dropped — the same cut applies to Slice D's
`activeAnnouncements`, below.

### Slice C: Change proposals

**Value**: The load-bearing slice — turns complaints into contributions.
**Path**: `Proposal { about, field, current_value, proposed_value, rationale, status }`.
**Acceptance criteria**:
- Propose a new value for a described field (description, tags, owners, custom properties).
- A proposal against a stale current value → `409` reporting the value changed underneath (reusing Epic 3's `If-Match` semantics).
- Accept applies the change; reject records a reason.
- Only an entity owner or a user with write permission may accept; others → `403`.
- A proposer without write permission may still propose — the entire point.
- Accepting bumps the version, attributed to the **proposer**, with the accepter recorded as approver (decision 3).
- Accepting an already-decided proposal → `409`.
- Proposals are listable per entity and per user.
**RED**: The attribution test is central: accept a proposal from user A, approved by user B, and assert `updatedBy` is A and the approver is B. The stale-value test is second. Mutator watch: attributing to the accepter must fail the first — it is the intuitive implementation and the wrong one.
**GREEN**: proposal entity, staleness check, application with attribution.
**REFACTOR**: attribution now diverges from "the principal making the request". Assess whether the facade's write path should take an explicit `attribution` distinct from `principal`, rather than special-casing proposals.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped, 6 August 2026.** No refactor was needed for the attribution split:
`Storage::update_asset` already takes `updated_by` as its own parameter, separate from
whichever principal is calling, so `accept_change_proposal` passes
`&proposal.proposed_by` directly — proven at the wire (`accepting_attributes_the_change_to_the_proposer_not_the_accepter`),
not just in a unit test, since a handler could serialise anything it likes. **Scope
cut**: only the `description` field can be applied by `accept` — the plan's own examples
(description, tags, owners, custom properties) are a larger surface than one slice
justifies, the same honest cut Epic 33's `apply_drift` already made for the same reason.
Proposing any other field is accepted (the record itself is general); accepting one is
refused with a named-field `400`.

### Slice D: Announcements

**Value**: "This table is being deprecated on the 30th" reaches people at the point of use.
**Path**: `Announcement { about, message, starts_at, ends_at, created_by }`.
**Acceptance criteria**:
- Create with a validity window; invalid window (`ends_at` before `starts_at`) → `400`.
- Active announcements returned on entity read via field selection.
- An announcement outside its window is not returned but is retained.
- An announcement on a container is visible on its descendants, flagged inherited.
- Only owners or users with write permission may create one.
- Listable and filterable by active state.
**RED**: Boundary tests at exactly `starts_at` and `ends_at` (inclusive start, exclusive end). Inheritance test asserting a schema announcement appears on its tables. Mutator watch: an off-by-one boundary must fail; missing inheritance must fail.
**GREEN**: entity, window evaluation, inheritance.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped, 6 August 2026.** Inheritance reuses the exact `ancestors_of` walk ownership
inheritance (Epic 11D) already uses, rather than storing the announcement once per
descendant — a schema-level announcement is visible on its tables by folding
`ancestors_of(table)` into the id list `active_announcements` queries, proven at the
wire with a real service→table hierarchy. The boundary is inclusive-start/exclusive-end
(`starts_at <= now AND ends_at > now`), proven at exactly both edges. Same field-selection
cut as Slice B: `GET /assets/{id}/announcements/active` is its own endpoint, not folded
into the entity read.

### Slice E: Reactions

**Value**: Cheap signal — "this answer helped" without a reply that adds noise.
**Path**: reactions on threads and posts.
**Acceptance criteria**: a fixed reaction set; one reaction of each type per user per post; repeating removes it (toggle); counts returned with posts; reacting to a deleted post → `400`.
**RED**: Toggle test asserting a repeat removes rather than duplicating. Mutator watch: non-toggling insert must fail it.
**GREEN**: reaction edges with toggle semantics.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped, 6 August 2026.** Toggle semantics proven at both the storage layer (direct
`has_reacted`/`add_reaction`/`remove_reaction` round trip) and the wire (`POST` twice,
assert `"add"` then `"remove"`, counts go 1 → 0). "Counts returned with posts" is
narrower than the plan's own wording: counts are their own endpoint
(`GET /posts/{id}/reactions`), not embedded in the `Post` returned by `list_posts` —
the same field-selection-shaped gap as Slices B and D, not fixed here either.

### Slice F: The activity feed

**Value**: One chronological view of everything that happened to an asset.
**Path**: merge Epic 3 change events with collaboration events.
**Acceptance criteria**:
- `GET /tables/{id}/activity` returns changes, threads, proposals, and announcements in one ordered, paginated stream.
- Filterable by activity type.
- A user-scoped feed (`/users/{id}/activity`) covers entities they own or follow.
- Ordering is stable under pagination — ties broken deterministically.
- Respects Epic 13 authorization: activity on unreadable entities is omitted.
- Efficient: the feed is not assembled by fanning out per entity.
**RED**: Stable-ordering test across a page boundary with identical timestamps. Authorization test asserting unreadable activity is absent. Mutator watch: an unstable tie-break must fail the first; missing authz filtering must fail the second.
**GREEN**: merged query, deterministic ordering, authz predicate.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped, 6 August 2026, with three scope cuts recorded rather than assumed away — the first closed 8 August 2026 (Phase 3 item 3.1)**:

1. ~~**No authorization filtering.**~~ **Closed.** `entity_activity` now checks the
   caller's `ViewBasic` predicate against the entity's own FQN before assembling
   any feed row, returning `NotFound` (not a distinguishable "forbidden") when
   denied — the same reasoning `agent_activity` and `authorized_lpg_elements`
   already use. Full write-up in "Explicitly deferred" below, which this scope
   cut has moved out of.
2. **No user-scoped feed.** `GET /users/{id}/activity` ("entities they own or follow")
   is not implemented. "Follow" names a watch mechanism this codebase does not have
   anywhere — not in this epic's own resolved decisions, no migration, no facade
   method — building one is a separate feature, not a plumbing gap.
3. **No `?kind=` filter and no real pagination.** The endpoint takes `limit` only; the
   plan's "filterable by activity type" and "ordering is stable under pagination" both
   assume query parameters that were not built. The sort key itself
   (`activity_sort_key`, `(occurred_at, id)` descending) *is* deterministic and unit-
   mutation-tested, so a paginated version would sort correctly once the offset
   parameter exists — what is missing is the parameter, not the ordering logic.

What **is** proven: the feed merges three independent sources (Epic 3's own
`asset_versions` plus five collaboration tables via one `collaboration_activity_for_entity`
query) for one entity in one call — not fanned out per row of a list — with a real
HTTP test asserting all four activity kinds (`change`, `threadStarted`,
`proposalCreated`, `proposalDecided`) appear from one request.

## Explicitly deferred (with destination)

- **Real-time delivery (WebSockets)** → polling plus events suffice for an hours-scale workflow.
- **External chat integration (Slack, Teams)** → needs the notification transport still deferred from Epic 11; the activity feed is the integration point when it arrives.
- **Nested reply threads** → one level plus reactions covers the need per decision 6.
- **Rich text / attachments** → markdown only; attachments need a storage story.
- **@-mentions with notification** → the parse is easy, the delivery is the deferred part; mentions can be recorded now and delivered when transport exists.
- **Task assignment / workflow** → proposals cover the catalog-specific case; general task management is another product.
- **A hard-delete asset endpoint** → the `ON DELETE CASCADE` on every collaboration
  table's `about`/`thread_id` FK is real schema, and the cascade itself is proven
  (a raw `DELETE FROM assets` in a repository test removes its threads), but
  `Storage` has no method that ever issues one — deletion is soft everywhere the API
  reaches, and `00g-operations.md`'s erasure story is still open. The FK is forward-
  looking design for when that lands, not dead weight.
- **Field-level embedding of collaboration data on entity reads** — unresolved-thread
  count, active announcements, and per-post reaction counts are each their own
  endpoint rather than riding along on `GET /assets/{id}`/`GET /tables/{id}` via field
  selection, as three of this plan's own acceptance criteria ask for. Epic 2's field
  selection mechanism is a fixed whitelist of columns already on the entity row; a
  computed cross-table value is a different extension shape it was not built for.
  Revisit as a small, focused slice once a second cross-table computed field wants the
  same thing (Epic 20's drift count is the other candidate).
- ~~**Authorization filtering on the activity feed**~~ — **Closed 8 August 2026
  (Phase 3 item 3.1)**. `entity_activity` now checks `predicate.admits(&asset.
  fully_qualified_name)` against a `ViewBasic` predicate before assembling any
  feed row, and returns `NotFound` — not a distinguishable "forbidden" — when
  denied, matching the reasoning `agent_activity` and `authorized_lpg_elements`
  already use for the same class of leak. Five new unit tests in
  `graph-owl-api/src/lib.rs`'s `the_activity_feed_respects_authorization`
  module prove: a scoped viewer gets `NotFound` for an entity outside their
  policy; the same viewer still sees an entity inside it; an admin and the
  system principal are unaffected; a genuinely unknown id is still
  `NotFound`. Mutation-tested (`--in-diff`, `--lib`): 2 caught, 1 unviable, 0
  survivors — critically, deleting the `!` in the admits check (which would
  invert allow/deny) is caught. The existing HTTP-level activity-feed test
  needed no change: its principal is promoted to real admin via raw SQL for
  an unrelated reason (`PUT /assets/{id}/owners` is admin-gated), so it was
  never exercising the restricted path this fix closes.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment — particularly the attribution split from Slice C.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. Authorization verified on every collaboration endpoint — user-generated content is the likeliest place for an authz gap.
