# Plan: Collaboration (Epic 35)
**Branch**: feat/collaboration
**Status**: Not started
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

- [ ] A user can start a thread on an entity or a field, and others can reply.
- [ ] A thread can be resolved, and resolved threads are filterable.
- [ ] A user can propose a field change; a steward can accept or reject it.
- [ ] An accepted proposal applies the change attributed to the proposer.
- [ ] An announcement displays on an entity for a validity window and then stops.
- [ ] An activity feed shows discussions and Epic 3 change events together, in order.
- [ ] Deleting an entity retains its threads for audit; hard delete removes them.

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

### Slice E: Reactions

**Value**: Cheap signal — "this answer helped" without a reply that adds noise.
**Path**: reactions on threads and posts.
**Acceptance criteria**: a fixed reaction set; one reaction of each type per user per post; repeating removes it (toggle); counts returned with posts; reacting to a deleted post → `400`.
**RED**: Toggle test asserting a repeat removes rather than duplicating. Mutator watch: non-toggling insert must fail it.
**GREEN**: reaction edges with toggle semantics.
**Done when**: criteria met, mutation report reviewed, commit approved.

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

## Explicitly deferred (with destination)

- **Real-time delivery (WebSockets)** → polling plus events suffice for an hours-scale workflow.
- **External chat integration (Slack, Teams)** → needs the notification transport still deferred from Epic 11; the activity feed is the integration point when it arrives.
- **Nested reply threads** → one level plus reactions covers the need per decision 6.
- **Rich text / attachments** → markdown only; attachments need a storage story.
- **@-mentions with notification** → the parse is easy, the delivery is the deferred part; mentions can be recorded now and delivered when transport exists.
- **Task assignment / workflow** → proposals cover the catalog-specific case; general task management is another product.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment — particularly the attribution split from Slice C.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. Authorization verified on every collaboration endpoint — user-generated content is the likeliest place for an authz gap.
