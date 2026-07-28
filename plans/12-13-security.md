# Plan: Authentication & Authorization (Epics 12–13)
**Branch**: feat/authn (Epic 12), feat/authz (Epic 13 — authorization & policy)
**Status**: **In progress** — console sign-in and authorization shipped (Demo 2); policy dry-run surface still open
**Depends on**: Epic 11 (`Principal` seam), Epic 11 (users and teams to attach roles to)
**Unblocks**: multi-team production use
**Crates**: **`graph-owl-authz`** (new — pure policy evaluator) · `graph-owl-core` (Principal) · `graph-owl-server` (JWT/JWKS middleware, extractor swap) · `graph-owl-api` (facade enforcement) · `graph-owl-storage-postgres` (predicate lowering to SQL)

Two epics in one plan because they share a domain and are meaningless apart: authentication without authorization means every verified user is an admin; authorization without authentication means policies apply to an anonymous principal.

## Goal

**Epic 12** — every request carries a verified identity, so `updated_by` names a person and the audit trail is worth keeping.
**Epic 13** — a user can only do what policy permits, so the catalog is safe for more than one team.

## Resolved decisions

1. **graph-owl validates tokens; it does not issue them.** No password storage, no session management, no login UI, no SSO redirect dance. Those belong to an identity provider. This keeps the highest-risk code out of the codebase entirely.
2. **The Epic 1 `Principal` seam is swapped, not extended.** Forty handlers already take a `Principal`; Epic 12 changes the extractor's implementation and nothing else. This is the payoff for a decision made nine epics earlier.
3. **`graph-owl-authz` is a pure crate.** Policy evaluation is `(principal, action, resource, policies) -> Decision` with no I/O, so it is exhaustively testable without a database. Fetching policies is the facade's job.
4. **Deny overrides allow, always.** A deny rule in any applicable policy wins over an allow in another. The alternative — policy ordering — makes reasoning about effective permissions impossible.
5. **Enforcement lives in the facade, not in handlers.** Every entry point (HTTP now, anything later) is covered by construction. Handler-level checks are one forgotten annotation away from a hole.
6. **Row-level filtering is compiled into the query**, never applied after retrieval. Post-filtering breaks pagination counts and leaks existence through result counts.
6a. **One predicate, lowered to four targets — and the intermediate representation is a deliverable, not a refactor.** The same policy must reach relational SQL (Epic 2), the search query (Epic 8), SPARQL over flakes (Epic 7), and Cypher over Bolt (Epics 7b, 7d). Four hand-written lowerings of the same policy is four chances to disagree, and a disagreement here is a data leak on whichever path is loosest. `graph-owl-authz` emits an **`AccessPredicate`** intermediate form; each adapter lowers that one structure. A **four-way equivalence test** — same principal, same corpus, four surfaces, identical visible sets — is the acceptance criterion, generalizing the three-way equivalence `07d-engine-bolt.md` already requires.
6b. **Policy inputs are read from the source of truth, never from the flake projection.** Flakes lag relational by design (`04-engine-triples.md` decision 1), so a tag or owner read from a flake can be stale by exactly the window in which access was revoked. Even for a query that runs over flakes, the predicate's inputs resolve against relational.
7. **JWT users are auto-provisioned on first sight** as `User` entities, so ownership works without a directory sync.
8. **Policy and schema data bypass policy evaluation.** Policies, shapes, and the predicate registry live in the graph like everything else, which creates a bootstrap loop: evaluating a policy requires reading the policy, which requires evaluating a policy. The cut is a `is_governance_flake()` predicate — flakes in the reserved namespaces (`0x0100–0x02FF`, per `04-engine-triples.md`) are readable without policy evaluation. **Readable, not writable**: writes to those namespaces are admin-only and go through the ordinary path. Without this, the first policy is unenforceable and the system deadlocks on its own governance.
9. **Authorization is over a named operation vocabulary, not over HTTP verbs.** A closed `MetadataOperation` enum — `ViewBasic`, `ViewDetails`, `ViewUsage`, `ViewLineage`, `Create`, `EditDescription`, `EditTags`, `EditOwners`, `EditLineage`, `EditCustomProperties`, `Delete`, `Restore` — is the unit a policy grants. `PUT /tables/{id}` maps to several of these depending on which fields changed, and "can this principal edit tags but not owners" is unanswerable if the vocabulary is `{GET, PUT, DELETE}`. Field-level permissions fall out of the same enum rather than needing a second mechanism. **Append-only**, by the same rule and for the same reason as `RelationshipType` (`01-api-conventions.md` decision 9) — these are persisted in policies.
10. **Decisions are cached per principal, invalidated on policy or membership change — never on a timer.** Evaluating every applicable policy on every field of every row in a page is the obvious performance cliff. A TTL cache is the obvious fix and the wrong one: it means a revoked permission stays live for the TTL, which is a security bug with a configurable duration. Cache on the `(principal, policy_epoch, membership_epoch)` triple so a policy edit invalidates by construction.
11. **An unmatched principal denies, and denies by construction.** A policy whose subject expression matches nothing must never fall through to allow. The evaluator grounds an unmatched identity against a value that cannot equal anything real, so "no rule matched" and "a rule matched and denied" reach the same outcome by the same code path rather than by a default branch someone can delete.

## Acceptance criteria (feature level)

**Epic 12 — authentication**
- [ ] An unauthenticated request → `401` in problem+json.
- [ ] A valid RS256 token populates `updated_by` with the real user.
- [ ] An expired, malformed, or wrong-audience token → `401` with a distinguishable `type`.
- [ ] A rotated signing key is picked up without restart.
- [ ] First sight of a subject auto-provisions a `User`.
- [ ] Connectors authenticate as bot users via API tokens.
- [ ] `/health`, `/ready`, and `/metrics` remain unauthenticated.

**Epic 13 — authorization & policy**
- [ ] Policies grant named `MetadataOperation` values; a policy granting `EditTags` does **not** permit an owner change through the same endpoint.
- [ ] A revoked permission takes effect on the next request — asserted by revoking mid-session, with **no** sleep in the test.
- [ ] Governance-namespace flakes are readable without policy evaluation and writable only by an admin.
- [ ] One `AccessPredicate` lowers to SQL, search, SPARQL, and Cypher; a **four-way equivalence test** asserts identical visible sets for one principal across all four.
- [ ] A permission revoked relationally is enforced on the **flake-backed** paths immediately, not after the projection catches up.
- [ ] A user without write permission → `403` on PATCH.
- [ ] List and search results omit entities the principal cannot read.
- [ ] A deny rule beats an allow rule from a different policy.
- [ ] Owners can edit what they own without an explicit grant.
- [ ] Admins bypass restrictions.
- [ ] Roles attach to users and teams; team roles inherit down the team tree.

## Epic 12 slices

### Slice A: Requests carry a verified identity

**Value**: The audit trail stops saying `system`.
**Path**: JWT bearer middleware; RS256 validation against a JWKS endpoint; the Epic 12 extractor swapped to return the verified principal.
**Acceptance criteria**:
- Valid token → request proceeds; `Principal` carries subject, name, email.
- Missing `Authorization` → `401`, `type: ".../unauthenticated"`.
- Expired token → `401`, `type: ".../token-expired"` (distinguishable, so a client knows to refresh rather than re-login).
- Wrong signature, wrong issuer, or wrong audience → `401`, each distinguishable.
- `Bearer` prefix required; a bare token → `401`.
- No handler signature changes — the seam holds.
**RED**: Table-driven test over valid / missing / expired / bad-signature / bad-issuer / bad-audience, asserting the distinct `type` per case. A test asserting handler signatures are unchanged from Epic 1. Mutator watch: validation that skips the audience or issuer check must fail its case; a check that accepts any signature must fail.
**GREEN**: JWKS client, validation middleware, extractor swap.
**REFACTOR**: assess whether validation belongs in a `graph-owl-authn` crate or stays in the server. Server — it is transport-coupled, unlike authz.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Key rotation is transparent

**Value**: An IdP rotating keys does not require a graph-owl restart or cause an outage.
**Path**: cached JWKS with TTL; refresh on unknown `kid`, rate-limited.
**Acceptance criteria**:
- Keys cached and reused; no fetch per request.
- Unknown `kid` triggers one refresh, then validation retries.
- Refresh is rate-limited — an unknown-`kid` flood does not become a JWKS DoS.
- JWKS unreachable with a warm cache → previously-valid tokens still validate.
- JWKS unreachable with a cold cache → `503`, not `401`; the token is not the problem, the service is.
**RED**: Test rotating the signing key mid-test and asserting the new token validates without restart. Test asserting an unknown-`kid` flood produces one fetch, not N. Mutator watch: fetch-per-request must fail the rate-limit test; `401`-on-cold-cache must fail the `503` assertion.
**GREEN**: cache with TTL, rate-limited refresh, cold/warm distinction.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Identities become users

**Value**: Ownership and roles work without a separate directory sync.
**Path**: on first sight of a subject, create a `User` from token claims.
**Acceptance criteria**:
- First request from a new subject creates a `User` with name and email from claims.
- Subsequent requests reuse it — one user, not one per request.
- A changed email in the token updates the user, bumping its version.
- Auto-provisioning is idempotent under concurrent first requests (no duplicate users).
- Provisioning is attributed to `system`, not to the user provisioning themselves.
**RED**: Concurrency test firing N simultaneous first-requests for the same subject, asserting exactly one user. Mutator watch: a non-atomic check-then-insert must fail it.
**GREEN**: upsert-on-conflict provisioning.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Machines authenticate as machines

**Value**: Connectors (Epic 11) get a real identity that is not a human's.
**Path**: API tokens issued to bot users; a separate `Authorization: Bearer got_…` scheme distinguished from JWTs by prefix.
**Acceptance criteria**:
- Token issuance, listing, and revocation for a bot user.
- Tokens are stored hashed, shown once at creation, never retrievable.
- A revoked token → `401` immediately.
- Optional expiry; expired → `401`.
- Token use updates a `last_used_at` for auditing.
- A bot principal is distinguishable from a human one in `Principal`.
**RED**: Test asserting the raw token is unrecoverable after creation and that only its hash is stored. Revocation test asserting immediate effect. Mutator watch: plaintext storage must fail the first; cached validation must fail the revocation test.
**GREEN**: token entity, hashing, revocation, dual-scheme extractor.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Epic 13 slices

### Slice E: Policies and roles exist

**Value**: The vocabulary of permission exists before anything is enforced.
**Path**: `graph-owl-authz` crate; `Policy { rules: Vec<Rule> }`, `Rule { effect, operations, resources, condition? }`; `Role { policies }`; CRUD for both.
**Acceptance criteria**:
- CRUD for policies and roles with the envelope.
- `effect` ∈ `allow|deny`; `operations` e.g. `Create|Read|Update|Delete|All`; `resources` are entity types or `*`.
- Roles attach to users and teams.
- A rule with an unknown operation or resource → `400`.
- Deleting a role in use → `409`.
- Seed policies created by migration: `OrgReader` (read all), `AssetOwner` (write owned), `Admin` (all).
**RED**: CRUD tests plus validation of unknown operations and resources. Mutator watch: absent validation must fail the unknown-operation case.
**GREEN**: crate, entities, seed migration.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Decisions are computed correctly

**Value**: The evaluation engine — pure, exhaustively testable, and the thing everything else trusts.
**Path**: `evaluate(principal, action, resource, policies) -> Decision` in `graph-owl-authz`. No I/O.
**Acceptance criteria**:
- Matching allow, no deny → `Allow`.
- Matching deny → `Deny`, even alongside a matching allow in another policy.
- No matching rule → `Deny` (default-deny).
- `*` resource and `All` operation match everything.
- Roles from a team apply to its members.
- Roles inherit down the team tree (a parent team's role applies to a child team's members).
- Owner condition: a principal owning the resource satisfies an owner-scoped rule.
- A disabled policy is ignored.
**RED**: An exhaustive truth table over allow/deny × matching/non-matching × direct/team/inherited. This is a pure function — the test table should be near-total. Mutator watch: allow-overrides-deny must fail; default-allow must fail the no-match case; single-level team lookup must fail the inheritance case.
**GREEN**: pure evaluator.
**REFACTOR**: assess whether `Decision` should carry *which* rule decided, for debuggability. Yes — "access denied" without a reason is unsupportable in production.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice G: Writes are enforced

**Value**: A user cannot delete another team's table.
**Path**: authorization check in the facade before every mutating operation.
**Acceptance criteria**:
- A principal without update permission → `403`, `type: ".../forbidden"`, naming the required operation and resource.
- An owner may update what they own without an explicit grant.
- An admin may do anything.
- The check precedes existence lookup where it does not leak: `403` for a resource the principal may not read, rather than `404` disclosing existence.
- Every mutating facade method is covered — verified structurally, not by inspection.
**RED**: Per-operation authorization tests. A test enumerating mutating facade methods and asserting each performs a check — so a newly added method without one fails CI. Mutator watch: a check that returns `Allow` unconditionally must fail every negative case.
**GREEN**: facade-level enforcement, structural coverage test.
**REFACTOR**: assess enforcing via a decorator around the `Catalog` rather than per-method calls — harder to forget. Prefer the decorator if it can carry enough context.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice H: Reads are filtered

**Value**: A user does not learn that a table exists by seeing it in a list they cannot open.
**Path**: policy compiled into list and search queries as a predicate, never post-filtered.
**Acceptance criteria**:
- Lists omit unreadable entities.
- `paging.total` reflects the filtered count.
- Search results are filtered identically, with facet counts respecting the filter.
- Direct `GET` of an unreadable entity → `403`, not `404` — but for an entity that does not exist at all, `404`.
- Filtering is applied in the query: a page of 25 readable entities requires one round trip, not 25 rejected fetches.
- An admin sees everything.
**RED**: Test with a mixed readable/unreadable corpus asserting page contents *and* `paging.total`. A test asserting the filtered query issues one database round trip. Mutator watch: post-retrieval filtering must fail the total-count assertion.
**GREEN**: `AccessPredicate` in `graph-owl-authz`, plus lowerings for Postgres and the search query. SPARQL and Cypher lowerings land with Epics 7 and 7b against the same structure.
**REFACTOR**: this is the hardest slice in the epic. The intermediate representation is decision 6a and is built here rather than assessed here — retrofitting it after two hand-written lowerings exist means rewriting both.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Being an identity provider** — never. Decision 1.
- **Column-level access control** → graph-owl governs access to *metadata*; the data plane is a different system's job.
- **Attribute-based policies beyond the condition expression** → add if the condition language proves insufficient.
- **SCIM / directory sync** → auto-provisioning covers the common case; add when group sync is required.
- **Policy simulation ("why was I denied?")** → the `Decision` carrying its deciding rule (Slice F refactor) is the foundation; a user-facing explain endpoint follows if support load justifies it.
- **Audit log as a separate store** → Epic 3 change events carry principal and diff; separate only if compliance demands it.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed. The authz evaluator (Slice F) is the highest-stakes pure function in the codebase; a surviving mutant there is a security bug.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. Run the `security-review` skill over both epics before merge.
5. Verify no token, key, or secret appears in any log line (extends Epic absorbed into 4 Slice C's redaction test).
