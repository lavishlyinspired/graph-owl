# Plan: Agent Capabilities (Epic 32) ★

**Branch**: feat/agent-capabilities
**Status**: Slices A–F built (core, storage, facade, HTTP, MCP write tools); no MCP transport, and only description proposals auto-apply
**Depends on**: Epic 14 (read surface, validated by real usage), Epic 31 (memory to write into)
**Crates**: `graph-owl-mcp` (write tools) · `graph-owl-authz` (AgentCapability, grants) · `graph-owl-core` (AgentGrant, RateLimit) · `graph-owl-api` (propose-by-default enforcement) — no new crates

## Goal

The write half of activation: agents propose and apply metadata updates, record investigations, and manage terms and tests — every action policy-checked, attributed, and reversible.

## Why write-back is late and separate

An agent with write access to the governance layer is a large trust surface. It should be built on a read surface that real usage has already validated, so the write tools match what agents actually needed rather than what seemed likely. Epic 14 shipping first is what makes this epic designable.

## Resolved decisions

1. **Agents write as themselves.** A distinct bot principal per agent, never a shared service account. Attribution is the entire basis of trust here, and a shared identity destroys it.
2. **Propose by default, apply by exception.** Most agent writes create a `Proposal` (Epic 35) for human acceptance. Direct application is permitted only for capabilities explicitly granted per agent — and never for deletion.
3. **Every agent write is reversible** through Epic 3's history. An agent action that cannot be undone is not shipped.
4. **Agents cannot grant themselves capability.** Policy changes are outside agent reach, permanently. An agent that can widen its own permissions has no permissions.
5. **Rate-limited and budgeted per agent.** A looping agent must not be able to author ten thousand memories. Limits are per principal, per capability, per window.
6. **Confidence is required on every agent assertion**, and below the Epic 31 threshold it becomes a proposal rather than an assertion regardless of granted capability.

## Implementation reference

```rust
pub enum AgentCapability {
    ProposeDescription, ProposeTags, ProposeOwner,
    ApplyDescription, ApplyTags,           // grantable, narrow
    RecordMemory, RecordInvestigation,
    CreateGlossaryTerm,                    // always creates Draft
    CreateQualityTest,
    LinkLineage,                           // always proposes
}

pub struct AgentGrant {
    pub agent: EntityReference,
    pub capabilities: Vec<AgentCapability>,
    pub scope: Option<ScopeRef>,           // domain or service restriction
    pub rate_limit: RateLimit,
    pub expires_at: Option<DateTime<Utc>>, // grants can be time-boxed
}
```

Notably absent and permanently so: any delete capability, any policy or role capability, any certification capability. Certification is a human accountability statement (Epic 26 decision 4); an agent granting it would void the concept.

### MCP write tools

| Tool | Default behaviour |
|---|---|
| `propose_metadata_change` | Creates a `Proposal` |
| `record_memory` | Asserts if confidence ≥ 0.8 and capability granted; else proposes |
| `record_investigation` | Asserts a `MemoryKind::Investigation` with findings and evidence |
| `create_glossary_term` | Creates as `Draft`, never `Approved` |
| `create_quality_test` | Creates the test; results still come from outside (Epic 30) |
| `link_lineage` | Always proposes — a wrong lineage edge propagates through impact analysis |

## Acceptance criteria

- [ ] Every agent write is attributed to a distinct bot principal.
- [ ] Capabilities are granted explicitly per agent, optionally scoped and time-boxed.
- [ ] Un-granted capability → refused with the required capability named.
- [ ] Proposals are the default; direct apply only where granted.
- [ ] No delete, policy, role, or certification capability exists.
- [ ] Rate limits are enforced per agent per capability.
- [ ] Every agent write is reversible via history.
- [ ] Low-confidence assertions degrade to proposals regardless of grant.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Grants and refusal

**Acceptance criteria**: `AgentGrant` CRUD, human-managed only; an un-granted capability is refused naming the capability required; scope restricts writes to a domain or service, and out-of-scope writes are refused; an expired grant refuses; grant changes are versioned and audited; **an agent attempting to modify a grant is refused regardless of any capability**.
**RED**: The self-grant test is the security-critical one — an agent calling the grant API must be refused even if it somehow holds every other capability. A scope test asserting an out-of-scope write is refused. Mutator watch: a capability check that permits grant modification must fail the self-grant test; ignoring scope must fail.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Propose by default

**Acceptance criteria**: `propose_metadata_change` creates a `Proposal` (Epic 35) with the agent as proposer, the change, and a rationale; acceptance applies with attribution to the **agent**, approver recorded separately (Epic 35's rule); a proposal against a stale value → `409`; an agent with only propose capability cannot apply; proposals are listable per agent so a steward can review an agent's track record.
**RED**: The attribution test: accept an agent's proposal and assert `updated_by` names the agent, with the human as approver. Getting this backwards destroys the audit trail and the incentive to review. Mutator watch: attributing to the approver must fail it.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Memory and investigation writing

**Acceptance criteria**: `record_memory` creates a `Memory` with `Authorship::Agent` including the session; confidence required; below 0.8 becomes a proposal even with `RecordMemory` granted; `record_investigation` records findings with evidence links to the assets and tests examined; an investigation with no evidence links → `422`; memories written by an agent are visibly agent-authored in retrieval (Epic 31's ranking weights them lower).
**RED**: The confidence-degradation test: a 0.6-confidence memory from a fully-granted agent must become a proposal, not an assertion — decision 6 overriding the grant. An evidence-required test. Mutator watch: asserting below threshold must fail the degradation test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Narrow direct-apply capabilities

**Acceptance criteria**: `ApplyDescription` and `ApplyTags` apply directly when granted; both bump the version, emit an event, and are revertible; applying to an entity the agent cannot read is refused (read gates write); direct apply respects the same Epic 5 validation as any write; no other apply capability exists — the enum is closed and a test asserts its exact membership.
**RED**: A test asserting the `AgentCapability` enum contains exactly the documented variants — so adding a delete capability requires changing a test that says why it must not exist. Mutator watch: an added capability must fail the membership test, which is the guard against scope creep in a security-sensitive enum.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Rate limits and budgets

**Acceptance criteria**: per-agent, per-capability, per-window limits; exceeding returns a refusal with `Retry-After`; limits are configurable per grant; a looping agent is stopped by the limit, not by exhausting the database; limit state survives restart; metrics per agent per capability.
**RED**: A loop test: an agent making N+1 writes in a window is refused on the N+1th, and the refusal does not consume further budget. A restart test asserting limit state persists. Mutator watch: in-memory-only limits must fail the restart test — a restart would reset a runaway agent's budget.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Reversibility and audit

**Acceptance criteria**: every agent write appears in Epic 3's history attributed to the agent; `GET /agents/{id}/activity` lists an agent's writes, paginated; any agent write is revertible to its prior state through the standard mechanism; a revert is attributed to the reverting principal, not the agent; an audit view shows agent writes accepted vs proposed vs refused, so an agent's reliability is measurable.
**RED**: A revert round-trip test asserting state matches pre-write exactly. An audit-completeness test asserting refused attempts are recorded too — an agent repeatedly attempting un-granted writes is a signal worth seeing. Mutator watch: recording only successful writes must fail the audit test.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Agent deletion capability** → never (decision 4 and the closed enum).
- **Agent-granted certification** → never; certification is human accountability (Epic 26).
- **Agents modifying policies or roles** → never.
- **Multi-agent coordination** (agents delegating to agents) → graph-owl is not the agent runtime.
- **Learned trust** (widening capability based on track record) → the audit data makes it measurable; automatic widening needs a human gate and is not planned.
- **Agent-initiated connector runs** → an operational trigger; add if a use case appears.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. 2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. **Run the `security-review` skill** — this epic grants write access to autonomous callers.
5. Verify the capability enum membership test exists and is documented (Slice D).
6. Verify an agent cannot modify its own grant under any capability (Slice A).
