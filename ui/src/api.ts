/** Types mirror the Rust wire contract. Epic 1 Slice J generates these from
 *  OpenAPI; until then they are hand-written and this file is the one place
 *  that drifts, which is why it is small and separate.
 *
 *  Token management uses the auth module's in-memory storage — no tokens
 *  reach localStorage, sessionStorage, or cookies. */

export type AssetKind = "service" | "database" | "schema" | "table" | "column";

export interface EntityVersion {
  major: number;
  minor: number;
}

export interface FieldChange {
  field: string;
  before: unknown;
  after: unknown;
}

export interface ChangeDescription {
  fieldsAdded: FieldChange[];
  fieldsUpdated: FieldChange[];
  fieldsDeleted: FieldChange[];
}

export type OwnerKind = "user" | "team";

/** A denormalized owner reference — Epic 11 Slices C and D. The server
 *  resolves `displayName` at read time, so a renamed team shows correctly
 *  here without a follow-up request. */
export interface EntityReference {
  id: string;
  kind: OwnerKind;
  displayName: string;
  /** Found by walking up the containment hierarchy rather than recorded on
   *  this entity itself. Always present — an older server that predates
   *  inheritance is not the same fact as "recorded directly", and collapsing
   *  the two would make a console read a 5,000-table catalog as fully owned
   *  when nobody has named an owner below the database. */
  inherited: boolean;
}

/** A team, as `GET /teams` returns it — Epic 11 Slices B and C. */
export interface Team {
  id: string;
  displayName: string;
  description: string | null;
  members: string[];
  /** The team this one reports into, `null` for a root. Always present: a console
   *  reading its absence cannot tell "top of the hierarchy" from "a server that
   *  does not know about nesting". */
  parentTeamId: string | null;
}

/** A glossary, as `GET /glossaries` returns it — Epic 24 Slice A. */
export interface Glossary {
  id: string;
  name: string;
  description: string | null;
  fullyQualifiedName: string;
  createdAt: string;
  updatedAt: string;
}

export type TermStatus = "draft" | "inReview" | "approved" | "deprecated";

/** A glossary term, as `GET /glossary-terms/{id}` returns it — Epic 24
 *  Slices A–C. `broader`/`narrower`/etc. are **not** carried here: they are
 *  a separate `/relations` read (Epic 42's `vocabularyTree` builds the
 *  hierarchy from that, not from this record). */
export interface GlossaryTerm {
  id: string;
  glossaryId: string;
  name: string;
  fullyQualifiedName: string;
  definition: string;
  status: TermStatus;
  synonyms: string[];
  abbreviations: string[];
  version: string;
  createdAt: string;
  updatedAt: string;
}

/** One SKOS relation on a term, as `GET /glossary-terms/{id}/relations`
 *  returns it — "every relation visible on this term, derived inverses
 *  included" (server route table), so a term's own `broader` list already
 *  reflects a relation another term declared pointing at it. */
export interface SkosRelation {
  kind: "broader" | "narrower" | "related" | "exactMatch" | "closeMatch";
  target: string;
}

/** A classification, as `GET /classifications` returns it — Epic 25. */
export interface Classification {
  id: string;
  name: string;
  description: string | null;
  mutuallyExclusive: boolean;
}

/** A label within a classification, as `GET /tags` returns it. */
export interface Tag {
  id: string;
  name: string;
  classificationId: string;
  fullyQualifiedName: string;
  description: string | null;
}

export interface TagUsage {
  total: number;
  byKind: { kind: string; count: number }[];
}

/** An accountability boundary, as `GET /domains` returns it — Epic 23. */
export interface Domain {
  id: string;
  name: string;
  fullyQualifiedName: string;
  parentId: string | null;
  description: string | null;
  domainType: string | null;
  experts: string[];
}

/** A consumable bundle of assets, spanning technical boundaries. */
export interface DataProduct {
  id: string;
  name: string;
  fullyQualifiedName: string;
  description: string | null;
  purpose: string | null;
  domainId: string | null;
}

/** How a pack may be used — Epic 33 decision 3. No "unknown" case: a pack
 *  the server accepted always has one of these. */
export type Licence =
  | { kind: "permissive"; name: string }
  | { kind: "attributionRequired"; name: string; notice: string }
  | { kind: "acknowledgementRequired"; name: string; notice: string };

/** An imported vocabulary, as `GET /ontology-packs` returns it — Epic 33. */
export interface OntologyPack {
  id: string;
  packId: string;
  version: string;
  licence: Licence;
  sourceUrl: string;
  /** The pack-owned glossary its terms landed in — a pack's terms are
   *  ordinary glossary terms, read through the identical
   *  `glossaryTerms`/`termRelations`/`termUsage` calls a real glossary
   *  uses, scoped to this id. No separate pack-term API is needed for
   *  browsing. */
  glossaryId: string;
  termCount: number;
}

// ---- Epic 42 Slice C: merge adjudication (Epic 17's resolution queue) ----

export type ReviewStatus = "pending" | "confirmed" | "rejected";

export type Evidence =
  | { kind: "exactFqn" }
  | { kind: "normalizedFqn" }
  | { kind: "exactName"; scope: string }
  | { kind: "nameSimilarity"; metric: string; value: number }
  | { kind: "structuralOverlap"; sharedColumns: number; total: number }
  | { kind: "sameParent" }
  | { kind: "sameSourceSystem" };

export type MergeDecidedBy =
  | { kind: "auto" }
  | { kind: "human"; userId: string }
  | { kind: "agent"; agentId: string; model: string };

export interface ResolutionCandidate {
  entity: string;
  fqn: string;
  score: number;
  evidence: Evidence[];
}

export type Resolution =
  | { kind: "new" }
  | { kind: "existing"; entity: string; confidence: number }
  | { kind: "ambiguous"; candidates: ResolutionCandidate[] };

export interface ReviewQueueEntry {
  id: string;
  target: string;
  candidate: string;
  score: number;
  evidence: Evidence[];
  status: ReviewStatus;
  /** Absent while `pending` — the server omits these fields entirely
   *  rather than sending `null`. */
  decidedBy?: MergeDecidedBy;
  decidedAt?: string;
  /** Present only once `rejected`. */
  reason?: string;
  createdAt: string;
}

export interface MergeRecord {
  id: string;
  canonical: string;
  merged: string;
  evidence: Evidence[];
  confidence: number;
  decidedBy: MergeDecidedBy;
  decidedAt: string;
  mergedAtT: number;
  splitAt?: string;
}

export interface BulkReviewOutcome {
  id: string;
  ok: boolean;
  problem: string | null;
}

// ---- Epic 42 Slice D: extraction claims (Epic 21) and drift (Epic 20) ----

export interface PendingClaim {
  id: string;
  runId: string;
  subject: string;
  predicate: string;
  object: string;
  confidence: number;
  /** The sentence the claim's raw evidence span was widened to — already
   *  server-side (`windowed_passage`). */
  passage: string;
  /** Offsets into `passage` itself, not into the original document. */
  span: [number, number];
}

export type ClaimDecision =
  | { outcome: "accept" }
  | { outcome: "edit"; subject: string; predicate: string; object: string }
  | { outcome: "reject"; reason: string };

export type DriftKind = "liveEdited" | "unapplied";
export type DriftStatus = "pending" | "applied" | "ignored";

export type ChangeProposalStatus = "pending" | "accepted" | "rejected";

export interface ChangeProposal {
  id: string;
  about: string;
  field: string;
  currentValue: string | null;
  proposedValue: string | null;
  rationale: string;
  status: ChangeProposalStatus;
  proposedBy: string;
  decidedBy: string | null;
  decidedAt: string | null;
  decisionReason: string | null;
  createdAt: string;
}

export type PrincipalKind = "user" | "service" | "system";

export interface WhoAmI {
  id: string;
  name: string;
  kind: PrincipalKind;
  roles: string[];
  isAdmin: boolean;
}

/** `"exactMatch"`/`"closeMatch"`/`"broadMatch"`/`"narrowMatch"` (a
 *  `skos:*Match`) or `"equivalentClass"` (`owl:equivalentClass`) — the same
 *  string doubles as both the wire predicate and, on a review entry, the
 *  discriminator for which `kind` a confirming `POST /alignments` must
 *  name (`graph_owl_ontology::alignment::Alignment::parts` writes the
 *  literal local name `"equivalentClass"` for that variant, so there is no
 *  separate `kind` field to read). */
export type AlignmentPredicate = "exactMatch" | "closeMatch" | "broadMatch" | "narrowMatch" | "equivalentClass";

export type AlignmentSourceKind = "curated" | "computed" | "human";

/** One entry in decision 4's confidence review band (`0.5..0.8`) — Epic 104
 *  Slice D's backend, put on the wire by Phase 3 item 3.14. Every field but
 *  `subject` is optional because the server reads them back from the
 *  reified node's own flakes rather than trusting they are always present
 *  (`graph-owl-api`'s own doc comment on `AlignmentReviewEntry`). */
export interface AlignmentReviewEntry {
  subject: string;
  left: string | null;
  right: string | null;
  predicate: AlignmentPredicate | null;
  sourceKind: AlignmentSourceKind | null;
  sourceDetail: string | null;
  confidence: number | null;
  lossyReverse: boolean | null;
}

/** The exact body `POST /alignments` accepts — mirrors
 *  `UpsertAlignmentRequest` in `graph-owl-server` field for field. */
export interface UpsertAlignmentRequest {
  kind: "match" | "equivalentClass";
  left: string;
  right: string;
  predicate?: string;
  source: { kind: AlignmentSourceKind; detail: string };
  confidence: number;
  lossyReverse?: boolean;
}

/** A reconciliation finding — Epic 105 P5.
 *
 *  **`PackFinding`, not `Finding`, because `Finding` is already taken** by
 *  governance (`./governance/queue`), which is a different thing: a
 *  severity-ranked policy violation the platform itself raises. This one is
 *  what a *domain pack's* rules concluded, and it cites the pack's own law
 *  rather than a platform policy. Two concepts that would be confusing to
 *  merge and worse to name identically.
 *
 *  **Deliberately carries `pack` rather than being a GST type.** The console
 *  has one findings queue serving every domain pack; a `GstFinding` here
 *  would be the first per-domain hardcoding in the frontend, and the second
 *  would follow within a release. */
export interface PackFindingEvidence {
  readonly subject: string;
  readonly predicate: string;
  readonly value: string;
}

export interface PackFinding {
  readonly id: string;
  readonly pack: string;
  readonly label: string;
  readonly subject: string;
  readonly summary: string;
  readonly governedBy: string;
  readonly evidence: readonly PackFindingEvidence[];
  readonly status: "pending" | "accepted" | "rejected";
  readonly detectedAt: string;
  readonly decidedBy?: string | null;
  readonly reason?: string | null;
}

/** One open obligation — Epic 105 P8's first real slice
 *  (`GET /packs/{pack}/obligations`, `plans/105h-obligation-calendar.md`).
 *  `anchor` is asserted (the event date a document actually recorded);
 *  `due` is derived (`anchor` plus the rule's period) — the console must
 *  never render the two identically, the same distinction `00f`
 *  non-negotiable 4 already requires of every computed value elsewhere. */
export interface Obligation {
  readonly pack: string;
  readonly label: string;
  readonly subject: string;
  readonly governedBy: string;
  readonly anchor: string;
  readonly due: string;
  readonly daysRemaining: number;
}

/** A pack's own registered *detector*, as `GET /packs/{pack}/finding-rules`
 *  returns it — the rule definition itself (what this pack knows how to
 *  find), distinct from `PackFinding` above (an actual detection an
 *  evaluation run produced). `evidence`/`similarity`/`span` are the rule's
 *  internal matching configuration, not rendered here — this type exists
 *  for "what can this pack detect", which only needs label/summary/
 *  governedBy. */
export interface FindingRuleDef {
  readonly pack: string;
  readonly label: string;
  readonly summary: string;
  readonly governedBy: string;
  readonly query: string;
  readonly subjectVar: string;
  readonly evidence: readonly unknown[];
  readonly similarity: unknown;
  readonly span: unknown;
}

/** A classification label applied to an asset — `GET /label-suggestions`
 *  returns the ones a classifier proposed and no human has vouched for yet.
 *
 *  **Domain-agnostic**: a classification is `{classification}.{tag}` over an
 *  asset FQN, and what those tags mean is the deployment's business, not the
 *  console's. */
export interface TagLabel {
  readonly tagFqn: string;
  readonly targetFqn: string;
  readonly labelType: string;
  readonly state: string;
  readonly appliedBy: string;
  readonly appliedAt: string;
}

/** A certification that has expired or is about to —
 *  `GET /recertification-queue`. */
export interface Certification {
  readonly id: string;
  readonly targetFqn: string;
  readonly typeName: string;
  readonly issuer: string;
  readonly issuedAt: string;
  readonly expiresAt: string;
  readonly status: string;
}

/** Which OWL profiles the loaded ontology fits, and which one reasoning was
 *  routed to — `GET /ontology/profile`.
 *
 *  **Domain-agnostic by construction.** The profile of an ontology is a fact
 *  about its axioms, not about its subject: GST's class hierarchy, a
 *  healthcare pack's and a banking pack's are all answered by the same
 *  detector. */
export interface OntologyProfiles {
  readonly rl: ProfileMembership;
  readonly el: ProfileMembership;
  readonly ql: ProfileMembership;
  readonly routing:
    | { readonly outcome: "route"; readonly profile: string }
    | { readonly outcome: "refused"; readonly firstOffendingAxiom: string; readonly reason: string };
}

export interface ProfileMembership {
  readonly member: boolean;
  /** **Why it is not a member, axiom by axiom.** A bare "not EL" is
   *  unactionable; the axiom that put it outside the profile is the thing an
   *  author can change. */
  readonly violations: readonly { readonly subject: string; readonly reason: string }[];
}

/** What OWL EL classification derived — `POST /reasoning/el/classify`.
 *
 *  **Different in kind from every finding this console shows.** A finding is
 *  the result of a query: something asserted, or something absent. A
 *  subsumption is a fact *nobody wrote down* that follows necessarily from the
 *  ones that were. */
export interface ElClassification {
  readonly subsumptions: readonly { readonly subclass: string; readonly superclass: string }[];
  /** Axioms the classifier could not use, with the construct that put them
   *  outside EL. Reported rather than skipped: a classification that silently
   *  ignored half the ontology would look complete and be wrong. */
  readonly refusedAxioms: readonly { readonly subject: string; readonly construct: string }[];
}

/** A pack's own console configuration — `GET /packs/{pack}/console`.
 *
 *  **What this replaced.** The reconciliation page's source list, its measures
 *  and its per-finding guidance were TypeScript constants, so the page knew how
 *  to render GST and only GST: a healthcare, banking or automotive pack would
 *  have had its data shown under GST's headings, or nothing at all. The page's
 *  *shape* — sources in, rules run, a statement out, exceptions to work — is
 *  genuinely domain-neutral and stays here; everything naming a domain now
 *  comes from the pack that owns it.
 *
 *  `404` for a pack with no `[console]` section, which is ordinary rather than
 *  exceptional and renders as an honest empty state. */
export interface PackConsoleConfig {
  /** Each rule's reviewer-facing wording, keyed by label — `[findings.guidance]`
   *  in the manifest. Delivered here because the finding-rule registry the
   *  loader posts has no field for it. */
  readonly guidance?: Readonly<
    Record<string, { title: string; meaning?: string; nextAction?: string; tone?: "warning" | "danger" | "info" }>
  >;
  readonly reconciliation?: {
    readonly label: string;
    readonly subtitle?: string;
    readonly currency?: string;
    readonly locale?: string;
    readonly matchKey?: string;
    readonly identity?: string;
    readonly recordNoun?: string;
    readonly measures?: readonly { readonly name: string; readonly label: string; readonly total?: boolean }[];
    /** Which predicate carries each role the page needs. The console knows the
     *  *roles* — a date, a period, the party a record is attributed to — and
     *  never their names, so a healthcare pack naming `claimDate`/`memberId`
     *  renders through the same query builder. */
    readonly fields?: {
      readonly date?: string;
      readonly period?: string;
      readonly party?: string;
      readonly partyId?: string;
      readonly partyName?: string;
    };
    readonly sources?: readonly {
      readonly key: string;
      readonly class: string;
      readonly label: string;
      readonly description?: string;
      readonly surface?: string;
      readonly role?: "opening" | "closing" | "evidence";
    }[];
  };
}

/** Structural analytics over one asset's bounded neighbourhood — Epic 105
 *  P10's projection, reachable from the console at last.
 *
 *  **Deliberately bounded, never whole-graph**: the projection cannot exceed
 *  the traversal walk's own node cap, so this answers "how connected is this
 *  neighbourhood" and never "how connected is the whole graph". */
export interface AssetAnalytics {
  /** Index-aligned with {@link inDegree} and {@link outDegree}. */
  readonly nodes: readonly string[];
  readonly inDegree: readonly number[];
  readonly outDegree: readonly number[];
  /** Nodes connected to nothing else in this neighbourhood. */
  readonly orphans: readonly string[];
  /** Which predicates were counted as graph structure — derived from the data,
   *  not a pack-specific list. */
  readonly edgeTypes: readonly string[];
  /** **Always present, never inferred from a count.** A truncated walk that
   *  looks complete is the failure this project refuses everywhere. */
  readonly truncated: boolean;
}

/** A node in a finding's evidence graph — Epic 105 P7
 *  (`GET /findings/{id}/evidence-graph`).
 *
 *  **`iri`, not `name`/`kind` like {@link GraphNode}.** A finding's subject
 *  can be in any pack's namespace, not only `dsc:`, so there is no catalog
 *  asset to resolve a display name from — the resolved IRI (when the
 *  deployment recognises the namespace) is the only label the server has. */
export interface EvidenceGraphNode {
  readonly id: string;
  readonly iri: string | null;
  /** Which source document(s) asserted this node's own flakes — Epic 105
   *  P7's provenance half. Empty when the node has no subject-position
   *  flakes of its own from any named import (referenced only as another
   *  subject's edge target), never absent — an empty array is a real
   *  answer, not a missing field. */
  readonly sources: readonly string[];
}

export interface EvidenceGraphEdge {
  readonly from: string;
  readonly to: string;
  readonly relationship: string;
  readonly derived?: boolean;
}

export interface EvidenceGraph {
  readonly nodes: readonly EvidenceGraphNode[];
  readonly edges: readonly EvidenceGraphEdge[];
  readonly truncated: boolean;
  /** Epic 105 P7's near-miss half — the second candidate a rule's
   *  similarity band suspects is the same entity as this finding's own
   *  subject, resolved by value rather than by traversal, because the
   *  missing edge between them is the entire premise of a rule like
   *  `GstinTransposition`. `null` for every finding whose rule has no such
   *  candidate — most of them, since near-miss linking is specific to a
   *  rule suspecting two subjects are one entity. */
  readonly nearMiss: EvidenceGraphNode | null;
}

export interface DriftItem {
  id: string;
  assetId: string;
  fullyQualifiedName: string;
  field: string;
  kind: DriftKind;
  liveValue: string | null;
  declaredValue: string | null;
  status: DriftStatus;
  reportedAt: string;
  /** Absent (the server omits the field, not `null`) until decided. */
  decidedAt?: string;
  decidedBy?: string;
  /** Present only once `ignored`. */
  reason?: string;
}

export interface Asset {
  id: string;
  kind: AssetKind;
  name: string;
  fullyQualifiedName: string;
  parentId: string | null;
  description: string | null;
  properties?: Record<string, unknown> | null;
  /** `[]` when unowned — an asset with no owner is a real, reportable state,
   *  not an absent field.
   *
   *  **Optional here even though the server always sends it.** A required type
   *  is a claim about the build, not about whichever server is answering: when
   *  this was declared required, an older server omitting the field took the
   *  whole asset page down. The optionality is what forces every reader to
   *  decide what an absent field means rather than assume `[]`. */
  owners?: EntityReference[];
  version: EntityVersion;
  updatedBy: string;
  changeDescription?: ChangeDescription | null;
  deleted: boolean;
  deletedAt?: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface AssetVersion {
  version: EntityVersion;
  snapshot: Asset;
  changeDescription?: ChangeDescription | null;
  updatedBy: string;
  updatedAt: string;
}

/** One facet bucket. `count` is over the *visible* set — the server computes
 *  facets after authorization, so a bucket never reveals a schema the reader
 *  may not see, nor how big it is. */
export interface Facet {
  value: string;
  count: number;
}

export type { LineageGraph, LineageNode, LineageEdge } from "./graph/lineage";
export type { Finding, Severity, Suggestion } from "./governance/queue";
export type { Explanation } from "./governance/explanation";
export type { Solution } from "./workbench/results";

/** What a SPARQL query returned, and what it cost. */
export interface SparqlResult {
  readonly rows: readonly import("./workbench/results").Solution[];
  readonly factsScanned: number;
  /** The budget cut the answer short. **Always present**, never inferred from
   *  the row count — a truncated answer that looks complete is the failure
   *  this project refuses everywhere. */
  readonly truncated: boolean;
  readonly asOf: number | null;
  /** What the engine decided to read, one entry per scan. */
  readonly plan: readonly string[];
  /** The projected variables, **in the order the query named them**. Solutions
   *  arrive as sorted maps, so this is the only place that order survives. */
  readonly variables: readonly string[];
  /** `SERVICE` endpoints that answered — Epic 101. Result-level, not
   *  per-row: `spareval` gives no hook to attribute one bound row to the
   *  call that produced it, only the query as a whole. */
  readonly federatedEndpoints: readonly string[];
  /** `SERVICE SILENT` endpoints that could not be reached. **Always
   *  present, never inferred from row count** — a silently-failed clause
   *  contributes no error and often no rows either, which is otherwise
   *  indistinguishable from "this endpoint genuinely has no matching data". */
  readonly silencedFailures: readonly string[];
  /** Alignments (Epic 104) this query's results crossed — the console
   *  criterion "on any cross-vocabulary result the alignment that made it
   *  reachable is inspectable". Result-level, not per-row, for the same
   *  reason `federatedEndpoints` is: the same [`AlignmentReviewEntry`]
   *  shape the review queue uses, reused rather than duplicated. Empty for
   *  the overwhelming majority of queries, which cross no alignment. */
  readonly alignmentsUsed: readonly AlignmentReviewEntry[];
}

// ---- Epic 42 Slice E: one asset as a property-graph node ----

export type PropertyValue =
  | { type: "boolean"; value: boolean }
  | { type: "integer"; value: number }
  | { type: "float"; value: number }
  | { type: "string"; value: string }
  | { type: "bytes"; value: number[] }
  | { type: "dateTime"; value: string }
  | { type: "duration"; value: number }
  | { type: "list"; value: PropertyValue[] }
  | { type: "elementRef"; value: string };

export interface LpgNode {
  elementId: string;
  labels: string[];
  properties: Record<string, PropertyValue>;
}

export type LossyMapping =
  | { kind: "refInProperty"; subject: string; predicate: string }
  | { kind: "namedGraphCollapse"; subject: string; graphs: string[] }
  | { kind: "typeNarrowed"; subject: string; predicate: string; from: "uuid" | "json" };

export interface MappingReport {
  lossy: LossyMapping[];
}

export interface LpgNodeView {
  node: LpgNode;
  report: MappingReport;
}

/** Epic 42 Slice G: the ontology editor's own request/response wire
 *  shapes. All three endpoints (`preview`/`dry-run`/`save`) always
 *  respond `200` — a bad document is a normal outcome, never an error,
 *  the same reasoning `AgentActivity`'s outcome field already carries. */
export type OntologyEditFormat = "turtle" | "ntriples" | "jsonld";

export interface OntologyPreviewTriple {
  s: string;
  p: string;
  o: string;
  oIsRef: boolean;
}

export type OntologyPreviewResult =
  | { kind: "syntaxError"; message: string; line: number | null; column: number | null }
  | { kind: "preview"; triples: OntologyPreviewTriple[]; declared: string[] };

export type OntologyDryRunResult =
  | { kind: "syntaxError"; message: string; line: number | null; column: number | null }
  | {
      kind: "checked";
      accepted: string[];
      rejected: [string, string][];
      newInferences: number;
    };

export type OntologySaveResult =
  | { kind: "syntaxError"; message: string; line: number | null; column: number | null }
  | { kind: "saved"; landed: string[]; skipped: string[]; rejected: [string, string][] };

/** Epic 32's closed capability set — see `AgentCapability::ALL`'s own doc
 *  comment for why nothing wider (delete, grants, policy, roles, certify)
 *  will ever be added. `apply*` writes directly; everything else proposes
 *  or records, never applying without a human. */
export type AgentCapability =
  | "proposeDescription"
  | "proposeTags"
  | "proposeOwner"
  | "applyDescription"
  | "applyTags"
  | "recordMemory"
  | "recordInvestigation"
  | "createGlossaryTerm"
  | "createQualityTest"
  | "linkLineage";

export interface EntityReference {
  id: string;
  kind: "user" | "team";
  displayName: string;
  inherited: boolean;
}

export interface AgentGrant {
  id: string;
  agent: EntityReference;
  capabilities: AgentCapability[];
  scope?: { fqnPrefix: string };
  rateLimit: { maxWrites: number; windowSeconds: number };
  expiresAt?: string;
  grantedBy: string;
  createdAt: string;
  updatedAt: string;
}

/** One line in an agent's history — **the outcome is what distinguishes a
 *  write-back from everything else**: `applied` changed the catalog
 *  directly, `proposed` only suggested a change a human has not yet
 *  decided, `refused` never took effect at all. Filtered server-side to
 *  what the caller may see (Epic 42 Slice F's own named RED test — an
 *  unfiltered activity log would leak `targetFqn`s the viewer cannot
 *  otherwise read). */
export interface AgentActivity {
  id: string;
  agentId: string;
  capability: AgentCapability;
  targetFqn: string;
  outcome: "applied" | "proposed" | "refused";
  refusal?: string;
  at: string;
}

export interface BoltSession {
  principal: string;
  connectedAt: string;
}

export type BoltStatus =
  | { enabled: false }
  | { enabled: true; maxConnections: number; activeConnections: number; sessions: BoltSession[] };

/** Both `null` for a storage backend with no partition split — a real,
 *  legitimate state (`TripleStore::partition_health`'s trait default),
 *  not an error. `oldestDeltaT` is a transaction time, the same `t` every
 *  as-of query uses — not wall-clock, no wall-clock companion exists. */
export interface PartitionHealth {
  deltaRows: number | null;
  oldestDeltaT: number | null;
}

/** Epic 14 Slice F — a registered outbound subscription. Never carries a
 *  secret: `register_outbound_webhook`'s response type has no field for
 *  one, by construction, not by omission at the handler. */
export interface OutboundWebhook {
  id: string;
  url: string;
  eventTypes: string[];
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

/** One queued delivery attempt. A successfully delivered row is deleted
 *  rather than marked — see `outboundWebhooks.ts`'s own doc comment — so
 *  every row reaching the console is still pending, retrying, or
 *  dead-lettered, never "delivered". */
export interface OutboundWebhookDelivery {
  id: string;
  webhookId: string;
  payload: unknown;
  attempt: number;
  nextAttemptAt: string;
  lastError: string | null;
  deadLettered: boolean;
  createdAt: string;
}

/** The stored violations queue, and the instant it reflects. */
export interface ValidationReport {
  readonly data: readonly import("./governance/queue").Finding[];
  /** The graph instant this reflects. `0` means no pass has ever run — which
   *  is a different statement from "nothing is wrong", and the only thing that
   *  makes an empty queue trustworthy. */
  readonly computedAtT: number;
  readonly total: number;
}

/** What one validation pass found. */
export interface ValidationRun {
  readonly conforms: boolean;
  readonly violations: number;
  readonly warnings: number;
  readonly info: number;
  readonly shapes: number;
  /** Shapes that could not be compiled. A pass over eighteen of twenty shapes
   *  produces a clean-looking report for the two that did not run. */
  readonly refusedShapes: number;
  readonly computedAtT: number;
}

/** What one reasoning run concluded. */
export interface ReasoningReport {
  readonly derived: number;
  readonly replaced: number;
  readonly iterations: number;
  readonly capped: string | null;
  readonly durationMs: number;
  /** Which strategy actually ran — Epic 97, Phase 3 item 3.11's own field. */
  readonly technique: "full" | "incremental";
  /** The transaction time this run accounts every retraction up to — the
   *  overlay-staleness stamp Epic 41 Slice G requires be shown. No
   *  wall-clock companion exists to convert it with (the same reason
   *  `PartitionHealth.oldestDeltaT` is shown raw), so a reader sees the
   *  watermark itself, not a fabricated age. */
  readonly maintainedTo: number;
  /** Routing was overridden (`?force=true`) against an ontology outside
   *  this run's own profile — never inferred from `ignoredAxioms` being
   *  non-empty, matching `SparqlOutcome.truncated`'s own always-present
   *  convention. */
  readonly partial: boolean;
  readonly ignoredAxioms: readonly { readonly subject: string; readonly reason: string }[];
}

/** A count of what an export would contain — Phase 3 item 3.15's preview. */
export interface ExportPreview {
  readonly nodes: number;
  readonly edges: number;
}
import type { LineageGraph } from "./graph/lineage";
import type { Explanation } from "./governance/explanation";

export interface ConnectorRun {
  id: string;
  connector: string;
  serviceName: string;
  startedAt: string;
  /** Null means the run never reported back — a crash, not a fast success. */
  finishedAt: string | null;
  created: number;
  skipped: number;
  failed: number;
  deleted: number;
  failures: unknown[];
  /** Why deletion detection declined. A refusal is a successful run that
   *  deliberately did nothing. */
  refusal: string | null;
  triggeredBy: string;
}

export interface SearchFacets {
  kind: Facet[];
  schema: Facet[];
}

export interface GraphNode {
  id: string;
  name: string;
  /** Null when the reader may not see the node. It stays in the picture as a
   *  bare node — removing it would claim a smaller neighbourhood than exists. */
  kind: AssetKind | null;
  fullyQualifiedName?: string;
}

export interface GraphEdge {
  from: string;
  to: string;
  relationship: string;
  /** The reasoner concluded this edge; nobody asserted it. Optional on the
   *  wire so an older server does not break the console — absent reads as
   *  asserted, which understates rather than overstates what was inferred. */
  derived?: boolean;
}

export interface GraphView {
  nodes: GraphNode[];
  edges: GraphEdge[];
  /** The walk hit its bound. Always shown — a partial picture presented as
   *  complete is the failure mode of every graph tool. */
  truncated: boolean;
}

export interface Overview {
  assets: { total: number; byKind: { kind: AssetKind; count: number }[] };
  documentation: { described: number; total: number };
  /** Null when no graph engine is configured — distinct from a graph of size
   *  zero, which is what a configured-but-empty projection looks like.
   *  `nodes` is comparable to `assets.total`: trailing it means the graph
   *  view is behind the entity store. */
  graph: { flakes: number; nodes: number; edges: number } | null;
  recentlyChanged: Asset[];
}

export interface Page<T> {
  data: T[];
  paging: { after: string | null };
}

/** RFC 9457. Clients branch on `type`, never on prose — 01-api-conventions.md. */
export interface Problem {
  type: string;
  title: string;
  status: number;
  detail: string;
  errors?: { field: string; code: string; detail: string }[];
}

export class ApiError extends Error {
  constructor(readonly problem: Problem) {
    super(problem.detail);
  }
}

const BASE = import.meta.env.DEV ? "/api" : "";

/** Imported lazily to avoid circular dependency — auth module imports from
 *  this file, and the refresh function is only needed during 401 handling. */
let _tryRefresh: (() => Promise<boolean>) | null = null;

export function setRefreshHandler(fn: () => Promise<boolean>) {
  _tryRefresh = fn;
}

/** The stable `type` URI for an unauthenticated request. Branching on this
 *  rather than on `status` keeps the client honest about *why* it was refused:
 *  a 401 from a proxy and a 401 from graph-owl mean different things. */
const UNAUTHENTICATED = "https://graph-owl.dev/errors/unauthenticated";
const TOKEN_EXPIRED = "https://graph-owl.dev/errors/token-expired";
const TOKEN_INVALID = "https://graph-owl.dev/errors/token-invalid";
const FORBIDDEN = "https://graph-owl.dev/errors/forbidden";

export function isUnauthenticated(error: unknown): boolean {
  return error instanceof ApiError && error.problem.type === UNAUTHENTICATED;
}

export function isForbidden(error: unknown): boolean {
  return error instanceof ApiError && error.problem.type === FORBIDDEN;
}

export function isTokenExpired(error: unknown): boolean {
  return error instanceof ApiError && error.problem.type === TOKEN_EXPIRED;
}

export function isTokenInvalid(error: unknown): boolean {
  return error instanceof ApiError && error.problem.type === TOKEN_INVALID;
}

/** Where the bearer token is *read from*, not where it is kept.
 *
 *  **This module deliberately does not hold the token.** It held its own copy
 *  once, beside the one in `auth/`, and nothing assigned to it — so every
 *  request went out unauthenticated after a completely successful sign-in, and
 *  the console showed the sign-in screen to a user who had just signed in.
 *
 *  Two modules each believing they own "the" token is what produced that, and
 *  a setter here would only make the two copies syncable rather than singular.
 *  `auth/` obtains, refreshes and clears the token; this asks it for the
 *  current one on every request, so there is exactly one value and it cannot
 *  go stale.
 *
 *  The default returns `null` rather than throwing: a server in open mode has
 *  no token and no auth module wired, and that is a valid deployment. */
let _tokenSource: () => string | null = () => null;

export function setTokenSource(source: () => string | null) {
  _tokenSource = source;
}

export function authToken(): string | null {
  return _tokenSource();
}

/** What this server will accept as a credential, read before anything else.
 *
 *  **Deliberately not routed through `request`.** This is the call that decides
 *  whether a credential is needed at all, so it must not be able to trigger the
 *  401-and-refresh path it is trying to configure — and against a server old
 *  enough to lack the endpoint it 404s with a body that is not problem+json,
 *  which `request` would fail to parse rather than report.
 *
 *  Rejects rather than returning a default. The caller owns the fallback,
 *  because "the server did not answer" and "the server said open" must not
 *  arrive at the same branch. */
export async function fetchAuthConfig(): Promise<unknown> {
  const response = await fetch(`${BASE}/auth/config`);
  if (!response.ok) throw new Error(`auth configuration unavailable: ${response.status}`);
  return (await response.json()) as unknown;
}

/** Downloads an export route's response as a file — Phase 3 item 3.15.
 *
 *  **Not a plain `<a href>`.** Every export route is Bearer-token
 *  authenticated the same way `request` above is, and a browser navigating
 *  directly to a URL sends no custom headers — a plain link would 401. This
 *  fetches with the same `Authorization` header `request` attaches, then
 *  hands the response off as a `Blob` for the browser's own save flow,
 *  which is the standard pattern for an authenticated SPA download.
 *
 *  The filename comes from the response's own `Content-Disposition` when
 *  the server sent one (every format but JSON graph, which has no file to
 *  name) — read from the real response rather than guessed client-side, so
 *  a server-side rename is never silently out of sync with what the
 *  browser saves it as.
 */
export async function downloadExport(path: string, fallbackFilename: string): Promise<void> {
  const token = authToken();
  const response = await fetch(`${BASE}${path}`, {
    headers: token ? { authorization: `Bearer ${token}` } : {},
  });
  if (!response.ok) {
    const problem = (await response.json()) as Problem;
    throw new ApiError(problem);
  }
  const disposition = response.headers.get("content-disposition");
  const match = disposition ? /filename="([^"]+)"/.exec(disposition) : null;
  const filename = match?.[1] ?? fallbackFilename;

  const blob = await response.blob();
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.append(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

async function request<T>(path: string, init?: RequestInit, retried?: boolean): Promise<T> {
  const token = authToken();
  const response = await fetch(`${BASE}${path}`, {
    ...init,
    headers: {
      "content-type": "application/json",
      ...(token ? { authorization: `Bearer ${token}` } : {}),
      ...init?.headers,
    },
  });
  if (!response.ok) {
    const problem = (await response.json()) as Problem;

    // 401 with a refresh handler and no prior retry: try silent refresh once.
    if (problem.status === 401 && _tryRefresh && !retried) {
      const refreshed = await _tryRefresh();
      if (refreshed) return request<T>(path, init, true);
    }

    // A denied or failed request must never surface as an empty result —
    // that teaches the user the data does not exist. Callers are responsible
    // for honouring this; see `isUnauthenticated`.
    throw new ApiError(problem);
  }
  // `204 No Content` carries no body — `delete_team`/`delete_policy` and
  // every other idempotent-delete endpoint return it. Calling `.json()` on
  // an empty body throws `Unexpected end of JSON input`, which every caller
  // typed `request<void>()` was silently exposed to.
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

export const api = {
  roots: () => request<Asset[]>("/assets/roots"),
  /** `asOf` is an RFC 3339 instant. The server reconstructs the entity from
   *  the graph at that transaction time — it is not a snapshot lookup. */
  asset: (id: string, asOf?: string | null) =>
    request<Asset>(
      `/assets/${id}${asOf ? `?asOf=${encodeURIComponent(asOf)}` : ""}`,
    ),
  children: (id: string) => request<Asset[]>(`/assets/${id}/children`),
  ancestors: (id: string) => request<Asset[]>(`/assets/${id}/ancestors`),
  search: (q: string, kind?: AssetKind) =>
    request<Page<Asset> & { facets: SearchFacets }>(
      `/assets/search?q=${encodeURIComponent(q)}${kind ? `&kind=${kind}` : ""}&limit=50`,
    ),
  /** The neighbourhood around an asset. Labels are resolved server-side —
   *  one statement per traversal is pointless if the client then makes one
   *  request per node. */
  graph: (id: string, hops: number, asOf?: string | null) =>
    request<GraphView>(
      `/assets/${id}/graph?hops=${hops}` +
        (asOf ? `&asOf=${encodeURIComponent(asOf)}` : ""),
    ),
  /** One request for the whole landing page. Six would render in six stages
   *  and show a different partial truth in each. */
  overview: () => request<Overview>("/overview"),
  stats: () => request<{ byKind: { kind: AssetKind; count: number }[] }>("/assets/stats"),
  versions: (id: string) => request<AssetVersion[]>(`/assets/${id}/versions`),
  updateAsset: (id: string, update: { description: string | null }) =>
    request<Asset>(`/assets/${id}`, { method: "PATCH", body: JSON.stringify(update) }),
  runPostgresConnector: (body: {
    connectionString: string;
    serviceName: string;
    includeSchemas?: string[];
  }) =>
    request<{ runId: string; created: number; skipped: number; failed: number; failures: unknown[] }>(
      "/connectors/postgres/runs",
      { method: "POST", body: JSON.stringify(body) },
    ),
  /** Recent runs, newest first. A run that leaves a record nobody can see is
   *  only half the feature — "did last night's sync work" has to be answerable
   *  without a database session. */
  connectorRuns: () => request<ConnectorRun[]>("/connectors/runs?limit=20"),
  /** The lineage graph around an asset. Both directions, because "what breaks
   *  if I change this" and "where did this number come from" are the same graph
   *  read in opposite directions. */
  lineage: (id: string, upstream: number, downstream: number) =>
    request<LineageGraph>(
      `/lineage/asset/${id}?upstream=${upstream}&downstream=${downstream}`,
    ),
  /** The violations queue. Stored results, not a fresh pass — this is polled,
   *  and recomputing a full-graph validation per poll would make the cheapest
   *  client the most expensive query in the system. */
  validationReport: (params: { focusNode?: string; severity?: string } = {}) => {
    const query = new URLSearchParams();
    if (params.focusNode) query.set("focusNode", params.focusNode);
    if (params.severity) query.set("severity", params.severity);
    query.set("limit", "200");
    return request<ValidationReport>(`/validation/report?${query}`);
  },
  runValidation: () => request<ValidationRun>("/validation/runs", { method: "POST" }),
  /** Accept a violation. Reason and expiry are both required and the server
   *  refuses without them — a waiver nobody can review is a violation deleted
   *  with extra steps. */
  waiveFinding: (body: {
    shape: string;
    focusNode: string;
    path: string | null;
    constraint: string;
    reason: string;
    expiresAt: string;
  }) =>
    request<{ id: string }>("/validation/waivers", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    }),
  revokeWaiver: (id: string) =>
    request<void>(`/validation/waivers/${id}`, { method: "DELETE" }),
  runReasoning: () => request<ReasoningReport>("/reasoning/runs", { method: "POST" }),
  /** A count of what an export would contain, without downloading anything
   *  — Phase 3 item 3.15. `path` is `previewPath(filters)` from
   *  `features/export/exportDialog.ts`, already carrying `?scope=`/`?asOf=`. */
  exportPreview: (path: string) => request<ExportPreview>(path),
  /** Run a SPARQL query. The budget is the server's, not ours — a client that
   *  could raise its own limit does not have one. */
  sparql: (query: string) =>
    request<SparqlResult>("/sparql", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ query }),
    }),
  /** One asset as a property-graph node — the other half of the Knowledge
   *  tab's toggle. `404` both when the asset does not exist and when it
   *  has not been graph-projected yet; there is nothing left to
   *  distinguish once the server's own authorization question is
   *  resolved. Not registered in the OpenAPI schema (a real, recorded
   *  gap — see the handler's own doc comment). */
  lpgNode: (assetId: string) => request<LpgNodeView>(`/assets/${assetId}/lpg-node`),
  /** The fast, as-the-author-types path — parse only. */
  ontologyEditorPreview: (format: OntologyEditFormat, document: string) =>
    request<OntologyPreviewResult>("/ontology-editor/preview", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ format, document }),
    }),
  /** The explicit "Check" button — shapes and reasoning. */
  ontologyEditorDryRun: (format: OntologyEditFormat, document: string) =>
    request<OntologyDryRunResult>("/ontology-editor/dry-run", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ format, document }),
    }),
  ontologyEditorSave: (format: OntologyEditFormat, document: string) =>
    request<OntologySaveResult>("/ontology-editor/save", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ format, document }),
    }),
  /** Every agent with a grant — admin-only server-side (`404` for anyone
   *  else, same tier as `/admin/bolt/status`), since a grant's own scope
   *  and rate limit are operational configuration, not a filtered read. */
  agentGrants: () => request<AgentGrant[]>("/agents/grants"),
  /** One agent's history, newest first. Server-filtered to what the caller
   *  may see — see `AgentActivity`'s own doc comment. */
  agentActivity: (agentId: string, after?: string) =>
    request<Page<AgentActivity>>(
      `/agents/${agentId}/activity${after ? `?after=${encodeURIComponent(after)}` : ""}`,
    ),
  /** Admin-only. `{enabled: false}` when the off-by-default `bolt` Cargo
   *  feature was not compiled in, or compiled in but never bound — a real,
   *  legitimate state, not an error. */
  boltStatus: () => request<BoltStatus>("/admin/bolt/status"),
  /** Epic 102's write-side partition backlog — read-only, never admin-gated
   *  (a row count and a transaction time cost nothing like a bulk move
   *  does). `deltaRows`/`oldestDeltaT` are both `null` for a backend with
   *  no partition split (`TripleStore::partition_health`'s trait default). */
  partitionHealth: () => request<PartitionHealth>("/admin/partition-health"),
  /** Admin-only. Folds up to `batchSize` rows of `flakes_delta` into
   *  `flakes_main`; manual-trigger only, no automatic scheduling exists.
   *  `/admin/*` query params are snake_case on the wire — unlike every
   *  documented route, this tier is deliberately undocumented in the
   *  OpenAPI contract (matching `/admin/restore`'s own `conflict_policy`/
   *  `regenerate_ids`), so Epic 1's camelCase convention was never applied
   *  here. */
  compactPartition: (batchSize?: number) =>
    request<{ moved: number }>(
      `/admin/compact${batchSize !== undefined ? `?batch_size=${batchSize}` : ""}`,
      { method: "POST" },
    ),
  /** Every registered outbound webhook subscription, without secrets —
   *  admin-only (`404` for anyone else, same tier as `/admin/bolt/status`),
   *  since a subscription's target URL is operational configuration. */
  outboundWebhooks: () => request<OutboundWebhook[]>("/admin/outbound-webhooks"),
  /** One subscription's queued and dead-lettered deliveries. A
   *  successfully delivered row is deleted, never marked "delivered" — see
   *  `outboundWebhooks.ts`'s own doc comment — so this is never a full
   *  history, only what is still pending or has given up. */
  outboundWebhookDeliveries: (webhookId: string) =>
    request<OutboundWebhookDelivery[]>(`/admin/outbound-webhooks/${webhookId}/deliveries`),
  /** What the reasoner concluded about one subject, as the last run stored it.
   *  Not a fresh pass — an asset page opens with this. */
  derivedAbout: (subject: string) =>
    request<{ s: string; p: string; o: string; t: number }[]>(
      `/reasoning/derived?subject=${encodeURIComponent(subject)}`,
    ),
  // ---- Admin: principals and teams (Epic 41 Slice F over Epic 11) ----
  teams: () => request<Team[]>("/teams"),
  upsertTeam: (body: {
    id: string;
    displayName: string;
    description?: string | null;
    members: string[];
    parentTeamId?: string | null;
  }) => request<Team>("/teams", { method: "POST", body: JSON.stringify(body) }),
  childTeams: (id: string) => request<Team[]>(`/teams/${encodeURIComponent(id)}/children`),
  /** `reassignToKind` is required alongside `reassignTo`: a user and a team can
   *  share an id, and guessing would transfer an estate to the wrong principal. */
  deletePrincipal: (
    kind: OwnerKind,
    id: string,
    reassign?: { to: string; kind: OwnerKind },
  ) => {
    const collection = kind === "team" ? "teams" : "users";
    const query = reassign
      ? `?reassignTo=${encodeURIComponent(reassign.to)}&reassignToKind=${reassign.kind}`
      : "";
    return request<void>(`/${collection}/${encodeURIComponent(id)}${query}`, {
      method: "DELETE",
    });
  },
  upsertUser: (id: string, body: { displayName: string; email?: string | null }) =>
    request<{ id: string; displayName: string; email: string | null; roles: string[] }>(
      `/users/${encodeURIComponent(id)}`,
      { method: "PUT", body: JSON.stringify(body) },
    ),
  /** Try a connection **before** saving it. A test that could only run against a
   *  stored config would confirm the credential after the mistake was made. */
  testConnector: (connector: string, body: { settings: Record<string, unknown>; secret?: string }) =>
    request<{ ok: boolean; detail?: string }>(
      `/connectors/${encodeURIComponent(connector)}/test`,
      { method: "POST", body: JSON.stringify(body) },
    ),
  /** Simulate a policy against a set of roles before it is saved. */
  dryRunPolicy: (policy: unknown, roles: string[]) =>
    request<import("./admin/policy").DryRun>("/policies/dry-run", {
      method: "POST",
      body: JSON.stringify({ policy, roles }),
    }),
  /** Every stored policy, with the roles it currently applies to. */
  policies: () =>
    request<{ policy: import("./admin/policy").Policy; roles: string[] }[]>("/policies"),
  /** Create or update a policy. `roles` replaces whatever it applied to
   *  before, not an addition to it. */
  savePolicy: (policy: unknown, roles: string[]) =>
    request<{ policy: import("./admin/policy").Policy; roles: string[] }>("/policies", {
      method: "POST",
      body: JSON.stringify({ policy, roles }),
    }),
  deletePolicy: (name: string) =>
    request<void>(`/policies/${encodeURIComponent(name)}`, { method: "DELETE" }),
  /** The `SERVICE` allow-list as currently configured. Read-only — Epic 101
   *  Slice E — there is nothing to write to yet; see the server route's own
   *  doc comment. */
  federationEndpoints: () => request<{ endpoints: string[] }>("/admin/federation"),
  /** What we know about a subject, best first, each with its staleness and
   *  score decomposition. Entity-scoped — the Knowledge tab's own read. */
  recallMemories: (subjectId: string, query = "", includeSuperseded = false) =>
    request<import("./memory/memory").RecalledMemory[]>(
      `/assets/${encodeURIComponent(subjectId)}/memories?q=${encodeURIComponent(query)}` +
        (includeSuperseded ? "&includeSuperseded=true" : ""),
    ),
  createMemory: (body: {
    kind: import("./memory/memory").Memory["kind"];
    content: string;
    summary?: string | null;
    confidence?: number;
    links: import("./memory/memory").MemoryLink[];
  }) => request<import("./memory/memory").Memory>("/memories", { method: "POST", body: JSON.stringify(body) }),
  supersedeMemory: (
    id: string,
    body: {
      kind: import("./memory/memory").Memory["kind"];
      content: string;
      summary?: string | null;
      confidence?: number;
      links: import("./memory/memory").MemoryLink[];
    },
  ) =>
    request<import("./memory/memory").Memory>(`/memories/${encodeURIComponent(id)}/supersede`, {
      method: "POST",
      body: JSON.stringify(body),
    }),
  retractMemory: (id: string, reason: string) =>
    request<import("./memory/memory").Memory>(`/memories/${encodeURIComponent(id)}/retract`, {
      method: "POST",
      body: JSON.stringify({ reason }),
    }),
  /** Cross-entity search over every memory, for administration. Retracted
   *  memories are excluded unless asked for — the working view should not
   *  be cluttered with what nobody believes any more. */
  searchMemories: (filter: {
    author?: string;
    minConfidence?: number;
    maxConfidence?: number;
    since?: string;
    until?: string;
    includeRetracted?: boolean;
  }) => {
    const params = new URLSearchParams();
    if (filter.author) params.set("author", filter.author);
    if (filter.minConfidence !== undefined) params.set("minConfidence", String(filter.minConfidence));
    if (filter.maxConfidence !== undefined) params.set("maxConfidence", String(filter.maxConfidence));
    if (filter.since) params.set("since", filter.since);
    if (filter.until) params.set("until", filter.until);
    if (filter.includeRetracted) params.set("includeRetracted", "true");
    return request<{ data: import("./memory/memory").Memory[]; total: number }>(
      `/memories?${params.toString()}`,
    );
  },
  /** What a connector needs configured, as its own JSON Schema — so a hundred
   *  connectors do not become a hundred hand-written forms. */
  connectorSchema: (connector: string) =>
    request<Record<string, unknown>>(`/connectors/${encodeURIComponent(connector)}/schema`),

  /** Why a fact holds, all the way down to the assertions under it. */
  explain: (s: string, p: string, o: string) =>
    request<Explanation>(
      `/reasoning/explain?s=${encodeURIComponent(s)}&p=${encodeURIComponent(p)}&o=${encodeURIComponent(o)}`,
    ),

  // ---- Epic 42 Slice A: the vocabulary browser (glossary first) ----

  glossaries: () => request<Glossary[]>("/glossaries"),
  glossary: (id: string) => request<Glossary>(`/glossaries/${id}`),
  glossaryTerms: (glossaryId: string) =>
    request<GlossaryTerm[]>(`/glossaries/${glossaryId}/terms`),
  glossaryTerm: (id: string) => request<GlossaryTerm>(`/glossary-terms/${id}`),
  /** A term's own relations — asserted and derived-inverse alike. The tree
   *  (`features/vocabulary/vocabularyTree.ts`) is built entirely from the
   *  `broader` entries this returns per term. */
  termRelations: (id: string) => request<SkosRelation[]>(`/glossary-terms/${id}/relations`),
  /** Every asset or column this term is attached to — the detail pane's
   *  "assets carrying the term" list. */
  termUsage: (id: string) => request<Page<string>>(`/glossary-terms/${id}/usage?limit=200`),

  // ---- Epic 42 Slice B: three more vocabularies, through the same browser ----

  classifications: () => request<Classification[]>("/classifications"),
  /** Every tag, or every tag under one classification. Unfiltered — not
   *  `GET /classifications/{id}/tags`, which only *creates* one — because
   *  the vocabulary browser renders every classification as a root in one
   *  tree and needs every tag to place as a child, not one classification's
   *  worth at a time. */
  tags: (classificationId?: string) =>
    request<Tag[]>(
      `/tags${classificationId ? `?classificationId=${encodeURIComponent(classificationId)}` : ""}`,
    ),
  /** Aggregate counts, not a list of FQNs — a tag's usage is reported by
   *  kind (`{kind, count}`), unlike a glossary term's `termUsage`, which
   *  the server can name individually because there are far fewer terms
   *  attached than assets carrying a common tag like `PII.Sensitive`. */
  tagUsage: (fqn: string) => request<TagUsage>(`/tags/${encodeURIComponent(fqn)}/usage`),
  domains: () => request<Page<Domain>>("/domains?limit=500"),
  /** Every data product in the catalog. There is no `GET
   *  /domains/{id}/data-products` — a product names its own `domainId`
   *  (or none), so the vocabulary browser's domain detail pane filters
   *  this client-side rather than the server offering a second index over
   *  the same relationship. */
  dataProducts: () => request<Page<DataProduct>>("/data-products?limit=500"),
  ontologyPacks: () => request<OntologyPack[]>("/ontology-packs"),
  /** `{data, total}` with `limit`/`offset` — not `Page<T>`'s cursor shape,
   *  which this endpoint does not use. Omitting `status` returns pending
   *  entries only, never "every status" — the server's own default. */
  reviewQueue: (params: {
    status?: ReviewStatus;
    kind?: AssetKind;
    minScore?: number;
    maxScore?: number;
    limit?: number;
    offset?: number;
  } = {}) => {
    const query = new URLSearchParams();
    if (params.status) query.set("status", params.status);
    if (params.kind) query.set("kind", params.kind);
    if (params.minScore !== undefined) query.set("minScore", String(params.minScore));
    if (params.maxScore !== undefined) query.set("maxScore", String(params.maxScore));
    query.set("limit", String(params.limit ?? 50));
    query.set("offset", String(params.offset ?? 0));
    return request<{ data: ReviewQueueEntry[]; total: number }>(`/resolution/queue?${query}`);
  },
  confirmReview: (id: string) =>
    request<Resolution>(`/resolution/queue/${id}/confirm`, { method: "POST" }),
  /** `204 No Content` on success — the server does not echo the decided
   *  entry back, so a caller wanting `decidedAt`/`decidedBy` refetches. */
  rejectReview: (id: string, reason: string) =>
    request<void>(`/resolution/queue/${id}/reject`, {
      method: "POST",
      body: JSON.stringify({ reason }),
    }),
  bulkDecideReview: (body: { ids: string[]; decision: "confirm" | "reject"; reason?: string }) =>
    request<{ data: BulkReviewOutcome[] }>("/resolution/queue/bulk", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  splitMerge: (id: string) => request<MergeRecord>(`/merges/${id}/split`, { method: "POST" }),
  /** A bare array, unlike every other queue here — no envelope, no
   *  pagination, no status filter. Always the pending-equivalent set;
   *  there is no way to ask this route for decided history. */
  extractionQueue: () => request<PendingClaim[]>("/extraction/queue"),
  /** `204 No Content` — unlike drift/proposal decisions, the server does
   *  not echo the decided claim back. */
  decideExtractionClaim: (id: string, decision: ClaimDecision) =>
    request<void>(`/extraction/claims/${id}/decision`, {
      method: "POST",
      body: JSON.stringify(decision),
    }),
  driftQueue: (params: { status?: DriftStatus; limit?: number; offset?: number } = {}) => {
    const query = new URLSearchParams();
    if (params.status) query.set("status", params.status);
    query.set("limit", String(params.limit ?? 50));
    query.set("offset", String(params.offset ?? 0));
    return request<{ data: DriftItem[]; total: number }>(`/drift?${query}`);
  },
  /** Every vocabulary this deployment understands beyond the shipped set.
   *  `declaredBy` carries `pack:<id>` for a pack-installed namespace, which is
   *  how the console discovers which domain packs exist without a new route. */
  namespaces: () => request<{ code: number; iri: string; declaredBy: string }[]>("/namespaces"),

  /** Land an RDF document in a named import graph. Admin-gated server-side. */
  importRdf: (source: string, turtle: string) =>
    request<{ landed: string[]; skipped: string[]; rejected: { subject: string; reason: string }[] }>(
      `/graph/import/rdf?source=${encodeURIComponent(source)}&format=turtle`,
      { method: "POST", body: turtle, headers: { "content-type": "text/turtle" } },
    ),

  /** Evaluate a pack's registered rules and record what they conclude —
   *  Epic 105 P5b. The native reconcile engine; no CLI involved. */
  reconcilePack: (pack: string) =>
    request<{ pack: string; evaluated: number; found: number; opened: number; alreadyOpen: number }>(
      `/packs/${encodeURIComponent(pack)}/reconcile`,
      { method: "POST" },
    ),

  /** What a pack knows how to detect, before any evaluation has run —
   *  the "what's inside this pack" view's own data source. Admin-gated
   *  server-side, matching `reconcilePack` above. */
  findingRules: (pack: string) =>
    request<FindingRuleDef[]>(`/packs/${encodeURIComponent(pack)}/finding-rules`),

  /** Every pack on disk this deployment could install but has not —
   *  `pack.toml` headers read server-side, cross-referenced against
   *  `GET /namespaces`'s own `declaredBy: "pack:<id>"` marker. */
  availablePacks: () => request<{ id: string; description: string }[]>("/packs/available"),

  /** Installs a pack by running the existing `graph-owl-load-pack` loader
   *  server-side, attributed to the caller's own admin session. `ok: false`
   *  is a real, expected outcome (a bad manifest, a rejected call) — not
   *  thrown — with `output` carrying the loader's own stdout/stderr so the
   *  admin can see why. */
  installPack: (pack: string) =>
    request<{ pack: string; ok: boolean; output: string }>(
      `/packs/${encodeURIComponent(pack)}/install`,
      { method: "POST" },
    ),

  findings: (params: { status?: string; pack?: string } = {}) => {
    const query = new URLSearchParams();
    if (params.status) query.set("status", params.status);
    if (params.pack) query.set("pack", params.pack);
    const suffix = query.toString();
    return request<PackFinding[]>(`/findings${suffix ? `?${suffix}` : ""}`);
  },
  obligationCalendar: (pack: string) =>
    request<Obligation[]>(`/packs/${encodeURIComponent(pack)}/obligations`),
  decideFinding: (id: string, status: "accepted" | "rejected", reason?: string) =>
    request<void>(`/findings/${id}/decision`, {
      method: "POST",
      body: JSON.stringify(reason ? { status, reason } : { status }),
    }),
  /** Degree centrality, connected components and orphan detection over one
   *  asset's bounded neighbourhood — `GET /assets/{id}/analytics`.
   *
   *  **The capability was built and only the agent could reach it.** Epic 105
   *  P10 wired `graph-owl-analytics` to the `analytics()` MCP tool and stopped
   *  there, so the console had no way to ask how connected anything was. The
   *  gap was a route, not an algorithm.
   *
   *  `nodes`, `inDegree` and `outDegree` are index-aligned — the client joins
   *  on position, and reordering either side attributes one node's
   *  connectivity to another. */
  /** A pack's declared console configuration, or `null` when it declares none
   *  — an ordinary answer, not a failure. */
  packConsole: (pack: string) =>
    request<PackConsoleConfig>(`/packs/${encodeURIComponent(pack)}/console`).catch(() => null),

  /** Labels a classifier proposed that nobody has vouched for yet. */
  labelSuggestions: () => request<readonly TagLabel[]>("/label-suggestions"),

  /** Certifications expired or expiring — the queue that says what has to be
   *  looked at again. */
  recertificationQueue: () => request<readonly Certification[]>("/recertification-queue"),

  /** Which OWL profiles the ontology fits, and where reasoning was routed. */
  ontologyProfile: () => request<OntologyProfiles>("/ontology/profile"),

  /** Classify the loaded ontology under OWL EL. Admin-gated server-side. */
  classifyEl: () => request<ElClassification>("/reasoning/el/classify", { method: "POST" }),

  /** **Why one class is classified under another** — the derivation path, as a
   *  list of steps. `404` when no such subsumption holds, which is an answer
   *  rather than a failure.
   *
   *  An entailment a reviewer cannot interrogate is worse than none: it looks
   *  authoritative and cannot be checked. The same argument `governedBy` makes
   *  for a finding. */
  explainSubsumption: (subclass: string, superclass: string) =>
    request<readonly string[]>(
      `/reasoning/el/explain?subclass=${encodeURIComponent(subclass)}&superclass=${encodeURIComponent(superclass)}`,
    ),

  assetAnalytics: (assetId: string, params: { hops?: number; maxNodes?: number } = {}) => {
    const query = new URLSearchParams();
    if (params.hops !== undefined) query.set("hops", String(params.hops));
    if (params.maxNodes !== undefined) query.set("maxNodes", String(params.maxNodes));
    const suffix = query.toString();
    return request<AssetAnalytics>(`/assets/${assetId}/analytics${suffix ? `?${suffix}` : ""}`);
  },

  /** The subgraph reachable from a finding's own subject — Epic 105 P7, the
   *  traversal half. Computed at answer time from whatever the graph
   *  actually contains, not the flat evidence list the rule's author named
   *  when it was written. */
  findingEvidenceGraph: (id: string) =>
    request<EvidenceGraph>(`/findings/${id}/evidence-graph`),
  applyDrift: (id: string) => request<DriftItem>(`/drift/${id}/apply`, { method: "POST" }),
  ignoreDrift: (id: string, reason: string) =>
    request<DriftItem>(`/drift/${id}/ignore`, {
      method: "POST",
      body: JSON.stringify({ reason }),
    }),
  /** Catalog-wide, unscoped by entity or proposer — Phase 3 item 3.2. */
  changeProposals: (params: { status?: ChangeProposalStatus; limit?: number; offset?: number } = {}) => {
    const query = new URLSearchParams();
    if (params.status) query.set("status", params.status);
    query.set("limit", String(params.limit ?? 50));
    query.set("offset", String(params.offset ?? 0));
    return request<{ data: ChangeProposal[]; total: number }>(`/change-proposals?${query}`);
  },
  acceptChangeProposal: (id: string) =>
    request<ChangeProposal>(`/change-proposals/${id}/accept`, { method: "POST" }),
  rejectChangeProposal: (id: string, reason: string) =>
    request<ChangeProposal>(`/change-proposals/${id}/reject`, {
      method: "POST",
      body: JSON.stringify({ reason }),
    }),
  /** The caller's own resolved identity — Phase 3 item 3.2's other named
   *  gap: the frontend had no way to answer "who am I" for a per-user
   *  fallback view. */
  whoAmI: () => request<WhoAmI>("/me"),
  /** A bare array, like `extractionQueue` — no envelope, no pagination, no
   *  status filter. Always the review-band set as it stands right now:
   *  there is no server-side "confirmed"/"rejected" history to page
   *  through, because confirming or rejecting an entry (via
   *  `upsertAlignment` below) either promotes it out of the band or
   *  retracts it outright — it does not transition to a stored status. */
  alignmentReviewQueue: () => request<AlignmentReviewEntry[]>("/alignments/review"),
  /** Admin-only server-side (`principal.is_admin`, enforced in
   *  `graph-owl-server`, not re-checked here). Writes one alignment,
   *  gated by decision 4's confidence bands regardless of source kind —
   *  `alignmentQueue.tsx`'s Confirm/Reject actions are both just specific
   *  bodies to this one call. */
  upsertAlignment: (body: UpsertAlignmentRequest) =>
    request<{ outcome: string }>("/alignments", {
      method: "POST",
      body: JSON.stringify(body),
    }),
};
