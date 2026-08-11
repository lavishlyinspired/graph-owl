//! Policy evaluation: `(principal, operation, resource) -> Decision`.
//!
//! Pure. No I/O, no database, no clock. Fetching policies is the facade's job,
//! which is what makes every rule here exhaustively testable — and a surviving
//! mutant in this crate is a security bug, not a style issue.

pub mod agent;

use serde::{Deserialize, Serialize};

/// What a principal is trying to do.
///
/// A named vocabulary rather than HTTP verbs: `PATCH /assets/{id}` maps to
/// several of these depending on which fields changed, and "may edit tags but
/// not owners" is unanswerable if the vocabulary is `{GET, PUT, DELETE}`.
///
/// **Append-only** — these are persisted inside policies, so a variant is never
/// removed or renamed (`01-api-conventions.md` decision 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MetadataOperation {
    /// See that the asset exists, and its name and kind.
    ViewBasic,
    /// See descriptions, properties, and column types.
    ViewDetails,
    /// See values classified as sensitive.
    ViewSensitive,
    Create,
    EditDescription,
    EditTags,
    EditOwners,
    Delete,
    Restore,
}

impl MetadataOperation {
    pub const ALL: [MetadataOperation; 9] = [
        MetadataOperation::ViewBasic,
        MetadataOperation::ViewDetails,
        MetadataOperation::ViewSensitive,
        MetadataOperation::Create,
        MetadataOperation::EditDescription,
        MetadataOperation::EditTags,
        MetadataOperation::EditOwners,
        MetadataOperation::Delete,
        MetadataOperation::Restore,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            MetadataOperation::ViewBasic => "viewBasic",
            MetadataOperation::ViewDetails => "viewDetails",
            MetadataOperation::ViewSensitive => "viewSensitive",
            MetadataOperation::Create => "create",
            MetadataOperation::EditDescription => "editDescription",
            MetadataOperation::EditTags => "editTags",
            MetadataOperation::EditOwners => "editOwners",
            MetadataOperation::Delete => "delete",
            MetadataOperation::Restore => "restore",
        }
    }
}

/// What a rule applies to. Deliberately coarse: a matcher that can express
/// anything is one nobody can reason about, and "what can Asha see" has to be
/// answerable by reading the policies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "value")]
pub enum ResourceMatcher {
    All,
    /// Assets whose FQN begins with this prefix — a service, a schema, a table.
    FqnPrefix(String),
    /// Assets carrying this classification (Epic 25).
    Tagged(String),
    /// Facts whose named graph begins with this prefix — `graph:import:gst`,
    /// most concretely (`plans/105y-named-graph-policy.md`). A dimension of
    /// its own, not folded into `FqnPrefix`: a named graph is not a catalog
    /// asset and has no FQN, and confusing the two would let an FQN-shaped
    /// grant on an identical-looking string leak into pack-data visibility
    /// (or vice versa) — see [`compile_named_graph`]'s own tests for the
    /// two-way check that this cannot happen.
    NamedGraph(String),
}

impl ResourceMatcher {
    #[must_use]
    pub fn matches(&self, resource: &Resource) -> bool {
        match self {
            ResourceMatcher::All => true,
            ResourceMatcher::FqnPrefix(prefix) => resource.fqn.starts_with(prefix.as_str()),
            ResourceMatcher::Tagged(tag) => resource.tags.iter().any(|t| t == tag),
            // A named graph is not a catalog `Resource` (it has no FQN, no
            // tags) — row-level evaluation for it happens where the row
            // actually lives (the flake's own `cx`), through
            // `compile_named_graph`'s predicate, not through this method.
            ResourceMatcher::NamedGraph(_) => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub fqn: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Effect {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub name: String,
    pub effect: Effect,
    pub operations: Vec<MetadataOperation>,
    pub resources: ResourceMatcher,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    pub name: String,
    pub rules: Vec<Rule>,
}

/// Who is asking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    pub id: String,
    pub roles: Vec<String>,
    /// Admins bypass policy. Explicit rather than modelled as a wildcard-allow
    /// policy, so "why can this person see everything" has one obvious answer
    /// instead of a rule buried in a set.
    pub is_admin: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
}

impl Decision {
    #[must_use]
    pub fn is_allowed(self) -> bool {
        matches!(self, Decision::Allow)
    }
}

/// Evaluates a request against a policy set.
///
/// **Deny overrides allow, and an unmatched request denies.** The second is the
/// one that matters: a policy set granting nothing must not fall through to
/// permitted, and there is no default-allow branch to accidentally delete —
/// `Deny` is simply what the function returns when nothing matched.
#[must_use]
pub fn evaluate(
    subject: &Subject,
    operation: MetadataOperation,
    resource: &Resource,
    policies: &[Policy],
) -> Decision {
    if subject.is_admin {
        return Decision::Allow;
    }

    let mut allowed = false;
    for rule in policies
        .iter()
        .flat_map(|policy| &policy.rules)
        .filter(|rule| rule.operations.contains(&operation) && rule.resources.matches(resource))
    {
        match rule.effect {
            // A single deny ends it. Ordering-based resolution would make
            // effective permissions impossible to reason about.
            Effect::Deny => return Decision::Deny,
            Effect::Allow => allowed = true,
        }
    }

    if allowed {
        Decision::Allow
    } else {
        Decision::Deny
    }
}

/// The compiled form a storage adapter lowers into its own query language.
///
/// One structure, several lowerings — SQL now, search and SPARQL later. Four
/// hand-written lowerings of one policy is four chances to disagree, and the
/// loosest one is the leak (`12-13-security.md` decision 6a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessPredicate {
    All,
    /// No row is visible. Distinct from an empty prefix list, which would match
    /// everything if lowered carelessly.
    Nothing,
    Fqn {
        allow_prefixes: Vec<String>,
        deny_prefixes: Vec<String>,
    },
    /// The named-graph counterpart of [`AccessPredicate::Fqn`] — same shape,
    /// a separate variant so a caller cannot check an FQN-scoped predicate
    /// against a named-graph identifier (or the reverse) and have it
    /// silently compile (`plans/105y-named-graph-policy.md`).
    NamedGraph {
        allow_prefixes: Vec<String>,
        deny_prefixes: Vec<String>,
    },
}

impl AccessPredicate {
    /// Whether one FQN survives the predicate. The reference semantics every
    /// lowering must agree with.
    #[must_use]
    pub fn admits(&self, fqn: &str) -> bool {
        match self {
            AccessPredicate::All => true,
            AccessPredicate::Nothing => false,
            AccessPredicate::Fqn {
                allow_prefixes,
                deny_prefixes,
            }
            | AccessPredicate::NamedGraph {
                allow_prefixes,
                deny_prefixes,
            } => {
                allow_prefixes.iter().any(|p| fqn.starts_with(p.as_str()))
                    && !deny_prefixes.iter().any(|p| fqn.starts_with(p.as_str()))
            }
        }
    }
}

/// Compiles a subject's policies into a predicate for one operation.
#[must_use]
pub fn compile(
    subject: &Subject,
    operation: MetadataOperation,
    policies: &[Policy],
) -> AccessPredicate {
    compile_prefixes(subject, operation, policies, PrefixDimension::Fqn)
}

/// The named-graph counterpart of [`compile`]: access to `graph:import:
/// {source}` (or any other named-graph identifier) as its own policy
/// decision, not derived from FQN visibility — Epic 105's own follow-up,
/// named but not built when the domain-neutrality work shipped
/// (`plans/105-domain-neutrality.md`, `plans/105y-named-graph-policy.md`).
#[must_use]
pub fn compile_named_graph(
    subject: &Subject,
    operation: MetadataOperation,
    policies: &[Policy],
) -> AccessPredicate {
    compile_prefixes(subject, operation, policies, PrefixDimension::NamedGraph)
}

/// Which resource dimension [`compile_prefixes`] is compiling for — the one
/// thing [`compile`] and [`compile_named_graph`] do not share, since an
/// FQN-shaped grant must never be read as a named-graph grant or the
/// reverse (`an_fqn_prefix_rule_does_not_grant_named_graph_access`,
/// `a_named_graph_rule_does_not_grant_fqn_access`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum PrefixDimension {
    Fqn,
    NamedGraph,
}

impl PrefixDimension {
    /// The rule's own prefix on this dimension, or `None` if the rule is
    /// shaped for the other dimension (or is a `Tagged` rule, which — the
    /// same reasoning `compile`'s own comment already gives for FQN
    /// compilation — needs a resource's tags that a row-level predicate does
    /// not carry, so it contributes nothing on *either* dimension rather
    /// than being compiled as unrestricted).
    fn prefix_of(self, resources: &ResourceMatcher) -> Option<&str> {
        match (self, resources) {
            (PrefixDimension::Fqn, ResourceMatcher::FqnPrefix(p))
            | (PrefixDimension::NamedGraph, ResourceMatcher::NamedGraph(p)) => Some(p.as_str()),
            _ => None,
        }
    }

    fn wrap(self, allow_prefixes: Vec<String>, deny_prefixes: Vec<String>) -> AccessPredicate {
        match self {
            PrefixDimension::Fqn => AccessPredicate::Fqn {
                allow_prefixes,
                deny_prefixes,
            },
            PrefixDimension::NamedGraph => AccessPredicate::NamedGraph {
                allow_prefixes,
                deny_prefixes,
            },
        }
    }
}

fn compile_prefixes(
    subject: &Subject,
    operation: MetadataOperation,
    policies: &[Policy],
    dimension: PrefixDimension,
) -> AccessPredicate {
    if subject.is_admin {
        return AccessPredicate::All;
    }

    let mut allow_all = false;
    let mut deny_all = false;
    let mut allow_prefixes = Vec::new();
    let mut deny_prefixes = Vec::new();

    for rule in policies
        .iter()
        .flat_map(|policy| &policy.rules)
        .filter(|rule| rule.operations.contains(&operation))
    {
        if matches!(rule.resources, ResourceMatcher::All) {
            match rule.effect {
                Effect::Allow => allow_all = true,
                Effect::Deny => deny_all = true,
            }
            continue;
        }
        let Some(prefix) = dimension.prefix_of(&rule.resources) else {
            continue;
        };
        match rule.effect {
            Effect::Allow => allow_prefixes.push(prefix.to_string()),
            Effect::Deny => deny_prefixes.push(prefix.to_string()),
        }
    }

    if deny_all {
        return AccessPredicate::Nothing;
    }
    if allow_all {
        return if deny_prefixes.is_empty() {
            AccessPredicate::All
        } else {
            // An empty prefix matches everything on this dimension.
            dimension.wrap(vec![String::new()], deny_prefixes)
        };
    }
    if allow_prefixes.is_empty() {
        return AccessPredicate::Nothing;
    }
    dimension.wrap(allow_prefixes, deny_prefixes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(fqn: &str) -> Resource {
        Resource {
            fqn: fqn.to_string(),
            tags: Vec::new(),
        }
    }

    fn analyst() -> Subject {
        Subject {
            id: "asha".to_string(),
            roles: vec!["risk-analyst".to_string()],
            is_admin: false,
        }
    }

    fn admin() -> Subject {
        Subject {
            id: "root".to_string(),
            roles: Vec::new(),
            is_admin: true,
        }
    }

    fn rule(effect: Effect, ops: &[MetadataOperation], resources: ResourceMatcher) -> Policy {
        Policy {
            name: "p".to_string(),
            rules: vec![Rule {
                name: "r".to_string(),
                effect,
                operations: ops.to_vec(),
                resources,
            }],
        }
    }

    #[test]
    fn no_policies_means_denied() {
        // The single most important case: a policy set granting nothing must
        // not fall through to permitted.
        assert_eq!(
            evaluate(
                &analyst(),
                MetadataOperation::ViewBasic,
                &resource("a.b"),
                &[]
            ),
            Decision::Deny
        );
    }

    #[test]
    fn an_allow_rule_permits_its_operation() {
        let policies = [rule(
            Effect::Allow,
            &[MetadataOperation::ViewBasic],
            ResourceMatcher::All,
        )];
        assert_eq!(
            evaluate(
                &analyst(),
                MetadataOperation::ViewBasic,
                &resource("a.b"),
                &policies
            ),
            Decision::Allow
        );
    }

    #[test]
    fn an_allow_does_not_leak_to_other_operations() {
        let policies = [rule(
            Effect::Allow,
            &[MetadataOperation::ViewBasic],
            ResourceMatcher::All,
        )];
        assert_eq!(
            evaluate(
                &analyst(),
                MetadataOperation::Delete,
                &resource("a.b"),
                &policies
            ),
            Decision::Deny,
            "permission to read is not permission to delete"
        );
    }

    #[test]
    fn deny_overrides_allow_in_either_order() {
        let permit = rule(
            Effect::Allow,
            &[MetadataOperation::ViewDetails],
            ResourceMatcher::All,
        );
        let refuse = rule(
            Effect::Deny,
            &[MetadataOperation::ViewDetails],
            ResourceMatcher::FqnPrefix("hdfc.core_banking".to_string()),
        );
        let target = resource("hdfc.core_banking.customers");

        for policies in [vec![permit.clone(), refuse.clone()], vec![refuse, permit]] {
            assert_eq!(
                evaluate(
                    &analyst(),
                    MetadataOperation::ViewDetails,
                    &target,
                    &policies
                ),
                Decision::Deny,
                "ordering must not decide the outcome"
            );
        }
    }

    #[test]
    fn a_deny_elsewhere_does_not_restrict_an_unrelated_resource() {
        let policies = [
            rule(
                Effect::Allow,
                &[MetadataOperation::ViewDetails],
                ResourceMatcher::All,
            ),
            rule(
                Effect::Deny,
                &[MetadataOperation::ViewDetails],
                ResourceMatcher::FqnPrefix("hdfc.core_banking".to_string()),
            ),
        ];
        assert_eq!(
            evaluate(
                &analyst(),
                MetadataOperation::ViewDetails,
                &resource("hdfc.payments.upi_transactions"),
                &policies
            ),
            Decision::Allow
        );
    }

    #[test]
    fn an_admin_bypasses_every_policy_including_denies() {
        let policies = [rule(
            Effect::Deny,
            &MetadataOperation::ALL,
            ResourceMatcher::All,
        )];
        assert_eq!(
            evaluate(
                &admin(),
                MetadataOperation::Delete,
                &resource("a"),
                &policies
            ),
            Decision::Allow
        );
    }

    #[test]
    fn a_prefix_match_is_a_prefix_not_a_substring() {
        let matcher = ResourceMatcher::FqnPrefix("hdfc.core".to_string());
        assert!(matcher.matches(&resource("hdfc.core_banking.customers")));
        assert!(
            !matcher.matches(&resource("other.hdfc.core_banking")),
            "a substring match would let an unrelated service inherit a policy"
        );
    }

    #[test]
    fn a_tag_matcher_matches_tags_not_names() {
        let matcher = ResourceMatcher::Tagged("PII".to_string());
        assert!(matcher.matches(&Resource {
            fqn: "a.b".to_string(),
            tags: vec!["PII".to_string()],
        }));
        assert!(!matcher.matches(&resource("a.PII")));
    }

    #[test]
    fn every_operation_has_a_distinct_wire_form() {
        let mut names: Vec<&str> = MetadataOperation::ALL.iter().map(|o| o.as_str()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "two operations share a wire form");
    }

    // ---- compilation ----

    #[test]
    fn an_admin_compiles_to_unrestricted() {
        assert_eq!(
            compile(&admin(), MetadataOperation::ViewBasic, &[]),
            AccessPredicate::All
        );
    }

    #[test]
    fn no_policies_compiles_to_nothing_not_to_all() {
        // The failure that silently exposes the whole catalog: an empty
        // allow-list lowered as "no restriction" rather than "no rows".
        assert_eq!(
            compile(&analyst(), MetadataOperation::ViewBasic, &[]),
            AccessPredicate::Nothing
        );
    }

    #[test]
    fn a_prefix_allow_compiles_to_that_prefix() {
        let policies = [rule(
            Effect::Allow,
            &[MetadataOperation::ViewBasic],
            ResourceMatcher::FqnPrefix("hdfc.payments".to_string()),
        )];
        assert_eq!(
            compile(&analyst(), MetadataOperation::ViewBasic, &policies),
            AccessPredicate::Fqn {
                allow_prefixes: vec!["hdfc.payments".to_string()],
                deny_prefixes: Vec::new(),
            }
        );
    }

    #[test]
    fn allow_all_with_a_deny_keeps_the_deny() {
        let policies = [
            rule(
                Effect::Allow,
                &[MetadataOperation::ViewBasic],
                ResourceMatcher::All,
            ),
            rule(
                Effect::Deny,
                &[MetadataOperation::ViewBasic],
                ResourceMatcher::FqnPrefix("hdfc.core_banking".to_string()),
            ),
        ];
        let predicate = compile(&analyst(), MetadataOperation::ViewBasic, &policies);
        assert!(predicate.admits("hdfc.payments.upi"));
        assert!(
            !predicate.admits("hdfc.core_banking.customers"),
            "collapsing allow-all to All would drop the deny entirely"
        );
    }

    #[test]
    fn a_blanket_deny_compiles_to_nothing_even_alongside_an_allow() {
        let policies = [
            rule(
                Effect::Allow,
                &[MetadataOperation::ViewBasic],
                ResourceMatcher::All,
            ),
            rule(
                Effect::Deny,
                &[MetadataOperation::ViewBasic],
                ResourceMatcher::All,
            ),
        ];
        assert_eq!(
            compile(&analyst(), MetadataOperation::ViewBasic, &policies),
            AccessPredicate::Nothing
        );
    }

    #[test]
    fn compilation_only_considers_the_requested_operation() {
        let policies = [rule(
            Effect::Allow,
            &[MetadataOperation::Delete],
            ResourceMatcher::All,
        )];
        assert_eq!(
            compile(&analyst(), MetadataOperation::ViewBasic, &policies),
            AccessPredicate::Nothing,
            "a grant on one operation must not compile into a predicate for another"
        );
    }

    // ---- named-graph compilation (Epic 105's own follow-up: `plans/00i-licensing.md`
    // is not the right doc; see `plans/105y-named-graph-policy.md`) ----

    fn named_graph_rule(effect: Effect, prefix: &str) -> Policy {
        rule(
            effect,
            &[MetadataOperation::ViewBasic],
            ResourceMatcher::NamedGraph(prefix.to_string()),
        )
    }

    #[test]
    fn an_admin_compiles_to_unrestricted_named_graph_access() {
        assert_eq!(
            compile_named_graph(&admin(), MetadataOperation::ViewBasic, &[]),
            AccessPredicate::All
        );
    }

    #[test]
    fn no_named_graph_policies_compiles_to_nothing_not_to_all() {
        // The identical failure mode `no_policies_compiles_to_nothing_not_to_all`
        // guards for FQN access: an empty allow-list must read as "no named
        // graph", not "every named graph".
        assert_eq!(
            compile_named_graph(&analyst(), MetadataOperation::ViewBasic, &[]),
            AccessPredicate::Nothing
        );
    }

    #[test]
    fn a_named_graph_allow_compiles_to_that_prefix() {
        let policies = [named_graph_rule(Effect::Allow, "graph:import:gst")];
        assert_eq!(
            compile_named_graph(&analyst(), MetadataOperation::ViewBasic, &policies),
            AccessPredicate::NamedGraph {
                allow_prefixes: vec!["graph:import:gst".to_string()],
                deny_prefixes: Vec::new(),
            }
        );
    }

    /// A rule shaped for FQN matching must not leak into named-graph
    /// compilation, and vice versa — the two dimensions are compiled
    /// independently, matching `compilation_only_considers_the_requested_operation`'s
    /// own "a grant on one axis must not compile into a predicate for
    /// another" property, generalized from operations to resource kind.
    #[test]
    fn an_fqn_prefix_rule_does_not_grant_named_graph_access() {
        let policies = [rule(
            Effect::Allow,
            &[MetadataOperation::ViewBasic],
            ResourceMatcher::FqnPrefix("graph:import:gst".to_string()),
        )];
        assert_eq!(
            compile_named_graph(&analyst(), MetadataOperation::ViewBasic, &policies),
            AccessPredicate::Nothing,
            "an FQN-shaped grant must not be read as a named-graph grant, even on an \
             identical-looking string"
        );
    }

    /// The mirror: a named-graph rule must not leak into FQN compilation.
    #[test]
    fn a_named_graph_rule_does_not_grant_fqn_access() {
        let policies = [named_graph_rule(Effect::Allow, "graph:import:gst")];
        assert_eq!(
            compile(&analyst(), MetadataOperation::ViewBasic, &policies),
            AccessPredicate::Nothing
        );
    }

    #[test]
    fn a_blanket_deny_compiles_named_graph_access_to_nothing_even_alongside_an_allow() {
        let policies = [
            rule(
                Effect::Allow,
                &[MetadataOperation::ViewBasic],
                ResourceMatcher::All,
            ),
            rule(
                Effect::Deny,
                &[MetadataOperation::ViewBasic],
                ResourceMatcher::All,
            ),
        ];
        assert_eq!(
            compile_named_graph(&analyst(), MetadataOperation::ViewBasic, &policies),
            AccessPredicate::Nothing
        );
    }

    /// `ResourceMatcher::All` grants both dimensions at once — an "allow
    /// everything" policy is not scoped to FQNs only, matching what its own
    /// name says.
    #[test]
    fn an_all_grant_compiles_to_unrestricted_named_graph_access_too() {
        let policies = [rule(
            Effect::Allow,
            &[MetadataOperation::ViewBasic],
            ResourceMatcher::All,
        )];
        assert_eq!(
            compile_named_graph(&analyst(), MetadataOperation::ViewBasic, &policies),
            AccessPredicate::All
        );
    }

    /// The property that makes the two paths trustworthy together: whatever the
    /// row-level predicate admits, a direct evaluation of the same request must
    /// also allow. A divergence is a leak on whichever path is looser.
    #[test]
    fn the_compiled_predicate_agrees_with_direct_evaluation() {
        let policies = [
            rule(
                Effect::Allow,
                &[MetadataOperation::ViewBasic],
                ResourceMatcher::All,
            ),
            rule(
                Effect::Deny,
                &[MetadataOperation::ViewBasic],
                ResourceMatcher::FqnPrefix("hdfc.core_banking".to_string()),
            ),
        ];
        let predicate = compile(&analyst(), MetadataOperation::ViewBasic, &policies);

        for fqn in [
            "hdfc.payments.upi_transactions",
            "hdfc.core_banking.customers",
            "hdfc.core_banking.customers.pan",
            "hdfc.lending.loan_accounts",
            "hdfc",
        ] {
            let direct = evaluate(
                &analyst(),
                MetadataOperation::ViewBasic,
                &resource(fqn),
                &policies,
            );
            assert_eq!(
                direct.is_allowed(),
                predicate.admits(fqn),
                "the two paths disagree on {fqn}"
            );
        }
    }
}

/// What actually determines an authorization decision.
///
/// **Keyed by the role *set*, not by the principal.** Two people holding the
/// same roles get the same predicate for the same operation, so keying by user
/// id would store one identical entry per user and make the cache scale with
/// headcount instead of with policy shape. It also means a thousand analysts
/// sharing one role warm the entry once between them.
///
/// `is_admin` is part of the key because [`compile`] short-circuits on it: an
/// admin's predicate does not depend on their roles at all, and two principals
/// differing only in that flag must not share an entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecisionKey {
    roles: String,
    is_admin: bool,
    operation: MetadataOperation,
}

impl DecisionKey {
    /// Build the key for a subject and operation.
    ///
    /// Roles are **sorted and joined with a separator that cannot occur in a
    /// role name**. Sorting makes the key order-independent, because
    /// `["reader", "steward"]` and `["steward", "reader"]` authorize
    /// identically and storing them separately would halve the hit rate for
    /// nothing. Duplicates collapse for the same reason.
    ///
    /// The separator is `\u{1f}` (unit separator) rather than a comma: a role
    /// literally named `a,b` would otherwise key identically to the pair
    /// `["a", "b"]`, which is a cache collision between two different
    /// authorization states — the one kind of collision that grants access
    /// somebody does not have.
    #[must_use]
    pub fn new(subject: &Subject, operation: MetadataOperation) -> Self {
        let mut roles: Vec<&str> = subject.roles.iter().map(String::as_str).collect();
        roles.sort_unstable();
        roles.dedup();
        Self {
            roles: roles.join("\u{1f}"),
            is_admin: subject.is_admin,
            operation,
        }
    }
}

/// Compiled predicates, held until something invalidates them.
///
/// **Invalidated by epoch, never by TTL** (`00g-operations.md`). A TTL makes
/// staleness the normal case: a revoked role stays live until the clock says
/// otherwise, and the window is invisible to the person who revoked it. An
/// epoch makes the revocation itself the trigger, so the cache is either
/// current or empty and there is no interval in which it is quietly wrong.
///
/// The cost of that choice is bluntness — one role change clears every entry,
/// including the ones it could not have affected. That is the right trade at
/// this size: recomputing a predicate is one indexed read and some string
/// work, and a precise invalidation would need a dependency graph from roles to
/// entries that can itself be wrong in the direction that matters.
pub struct DecisionCache {
    inner: std::sync::Mutex<CacheInner>,
    capacity: usize,
}

struct CacheInner {
    entries: std::collections::HashMap<DecisionKey, (AccessPredicate, u64)>,
    /// Monotonic use counter, for least-recently-used eviction. Not a clock:
    /// wall time would make eviction depend on how fast requests arrive.
    tick: u64,
}

/// 1024 entries.
///
/// The cache is keyed by role *set*, so its size is bounded by the number of
/// distinct role combinations actually in use times the operation count — not
/// by user count, which is the number that grows. A thousand entries covers
/// well over a hundred role combinations across every operation in
/// [`MetadataOperation`], and at a few hundred bytes for a predicate carrying a
/// handful of FQN prefixes that sits comfortably inside the 32 MB line
/// `00g-operations.md` budgets for authorization decisions.
const DEFAULT_CAPACITY: usize = 1024;

impl Default for DecisionCache {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }
}

impl DecisionCache {
    /// # Panics
    ///
    /// If `capacity` is zero — a cache that can hold nothing is a bug at the
    /// call site, not a configuration to honour silently.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(
            capacity > 0,
            "a decision cache must be able to hold an entry"
        );
        Self {
            inner: std::sync::Mutex::new(CacheInner {
                entries: std::collections::HashMap::new(),
                tick: 0,
            }),
            capacity,
        }
    }

    /// The cached predicate, if one is current.
    #[must_use]
    pub fn get(&self, key: &DecisionKey) -> Option<AccessPredicate> {
        let mut inner = self.inner.lock().ok()?;
        inner.tick += 1;
        let tick = inner.tick;
        let (predicate, last_used) = inner.entries.get_mut(key)?;
        *last_used = tick;
        Some(predicate.clone())
    }

    /// Remember a compiled predicate, evicting the least recently used if full.
    ///
    /// **One surviving mutant is accepted here and left documented**: replacing
    /// the tick's `+=` with `*=` freezes the counter, so entries inserted
    /// without an intervening read all carry the same recency and eviction
    /// falls back to whatever the map iterates first. That is a genuine
    /// behaviour change — arbitrary instead of least-recently-used — but it is
    /// only observable *statistically*, because a tie-break can pick the right
    /// victim by chance. Killing it deterministically would mean exposing the
    /// recency counter purely so a test could read it, which trades a real
    /// encapsulation boundary for a mutation score.
    ///
    /// The read-driven path — the one that dominates, since hits outnumber
    /// fills — is unaffected and is covered by
    /// `the_coldest_entry_is_evicted_and_the_hot_one_survives`.
    pub fn insert(&self, key: DecisionKey, predicate: AccessPredicate) {
        let Ok(mut inner) = self.inner.lock() else {
            // A poisoned lock means another thread panicked holding it. Not
            // caching is always safe — the caller recomputes — so this degrades
            // rather than propagating a panic into an authorization check.
            return;
        };
        inner.tick += 1;
        let tick = inner.tick;
        if inner.entries.len() >= self.capacity
            && !inner.entries.contains_key(&key)
            && let Some(coldest) = inner
                .entries
                .iter()
                .min_by_key(|(_, (_, last_used))| *last_used)
                .map(|(k, _)| k.clone())
        {
            inner.entries.remove(&coldest);
        }
        inner.entries.insert(key, (predicate, tick));
    }

    /// Drop every decision.
    ///
    /// Called when anything a decision was computed *from* changes — a role
    /// assignment, a policy edit. Wholesale rather than selective: see the type
    /// documentation.
    pub fn invalidate(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.entries.clear();
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().map_or(0, |inner| inner.entries.len())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod decision_cache_tests {
    use super::*;

    fn subject(roles: &[&str], is_admin: bool) -> Subject {
        Subject {
            id: "someone".to_string(),
            roles: roles.iter().map(ToString::to_string).collect(),
            is_admin,
        }
    }

    fn prefixes(allow: &[&str]) -> AccessPredicate {
        AccessPredicate::Fqn {
            allow_prefixes: allow.iter().map(ToString::to_string).collect(),
            deny_prefixes: Vec::new(),
        }
    }

    mod what_makes_two_requests_the_same_question {
        use super::*;

        /// Two people holding the same roles authorize identically. Keying by
        /// user id would store one identical entry each and make the cache
        /// scale with headcount rather than with policy shape.
        #[test]
        fn two_principals_with_the_same_roles_share_a_key() {
            let a = DecisionKey::new(&subject(&["steward"], false), MetadataOperation::ViewBasic);
            let b = DecisionKey::new(&subject(&["steward"], false), MetadataOperation::ViewBasic);

            assert_eq!(a, b);
        }

        #[test]
        fn role_order_does_not_matter() {
            let a = DecisionKey::new(
                &subject(&["reader", "steward"], false),
                MetadataOperation::ViewBasic,
            );
            let b = DecisionKey::new(
                &subject(&["steward", "reader"], false),
                MetadataOperation::ViewBasic,
            );

            assert_eq!(a, b);
        }

        #[test]
        fn a_repeated_role_is_the_same_set() {
            let a = DecisionKey::new(
                &subject(&["steward", "steward"], false),
                MetadataOperation::ViewBasic,
            );
            let b = DecisionKey::new(&subject(&["steward"], false), MetadataOperation::ViewBasic);

            assert_eq!(a, b);
        }

        /// **The collision that would grant access nobody has.** With a comma
        /// separator, a role literally named `a,b` keys identically to holding
        /// the two roles `a` and `b` — two different authorization states
        /// sharing one cache entry.
        #[test]
        fn a_role_name_cannot_impersonate_a_pair_of_roles() {
            let pair = DecisionKey::new(&subject(&["a", "b"], false), MetadataOperation::ViewBasic);
            let comma = DecisionKey::new(&subject(&["a,b"], false), MetadataOperation::ViewBasic);

            assert_ne!(pair, comma);
        }

        #[test]
        fn different_operations_are_different_questions() {
            let read =
                DecisionKey::new(&subject(&["steward"], false), MetadataOperation::ViewBasic);
            let edit = DecisionKey::new(
                &subject(&["steward"], false),
                MetadataOperation::EditDescription,
            );

            assert_ne!(read, edit);
        }

        /// `compile` short-circuits on `is_admin`, so an admin's predicate does
        /// not depend on their roles. Two principals differing only in that
        /// flag must not share an entry.
        #[test]
        fn an_admin_and_a_non_admin_with_the_same_roles_are_different_questions() {
            let admin =
                DecisionKey::new(&subject(&["steward"], true), MetadataOperation::ViewBasic);
            let user =
                DecisionKey::new(&subject(&["steward"], false), MetadataOperation::ViewBasic);

            assert_ne!(admin, user);
        }

        #[test]
        fn no_roles_is_a_key_of_its_own_and_not_a_match_for_any_role() {
            let none = DecisionKey::new(&subject(&[], false), MetadataOperation::ViewBasic);
            let some =
                DecisionKey::new(&subject(&["steward"], false), MetadataOperation::ViewBasic);

            assert_ne!(none, some);
        }
    }

    mod remembering_and_forgetting {
        use super::*;

        #[test]
        fn a_stored_decision_comes_back() {
            let cache = DecisionCache::default();
            let key = DecisionKey::new(&subject(&["steward"], false), MetadataOperation::ViewBasic);

            cache.insert(key.clone(), prefixes(&["hdfc-core"]));

            assert_eq!(cache.get(&key), Some(prefixes(&["hdfc-core"])));
        }

        #[test]
        fn a_question_never_asked_is_a_miss() {
            let cache = DecisionCache::default();
            let key = DecisionKey::new(&subject(&["steward"], false), MetadataOperation::ViewBasic);

            assert_eq!(cache.get(&key), None);
        }

        /// **The security property.** A revoked role must not survive in the
        /// cache. Invalidation is what makes the revocation take effect, and it
        /// happens on the change rather than on a clock.
        #[test]
        fn invalidation_forgets_everything() {
            let cache = DecisionCache::default();
            let key = DecisionKey::new(&subject(&["steward"], false), MetadataOperation::ViewBasic);
            cache.insert(key.clone(), AccessPredicate::All);

            cache.invalidate();

            assert_eq!(cache.get(&key), None);
            assert!(cache.is_empty());
        }

        /// The negative for `is_empty`: a cache holding something must say so.
        /// Without this, "always empty" satisfies every other assertion here.
        #[test]
        fn a_cache_holding_a_decision_is_not_empty() {
            let cache = DecisionCache::default();
            assert!(cache.is_empty(), "nothing stored yet");

            cache.insert(
                DecisionKey::new(&subject(&["steward"], false), MetadataOperation::ViewBasic),
                AccessPredicate::All,
            );

            assert!(!cache.is_empty());
            assert_eq!(cache.len(), 1);
        }

        /// And the negative: invalidation must be an event, not a default. A
        /// cache that forgot on every read would be correct and useless, and
        /// the test above alone cannot tell the two apart.
        #[test]
        fn a_read_does_not_forget() {
            let cache = DecisionCache::default();
            let key = DecisionKey::new(&subject(&["steward"], false), MetadataOperation::ViewBasic);
            cache.insert(key.clone(), AccessPredicate::All);

            assert!(cache.get(&key).is_some());
            assert!(
                cache.get(&key).is_some(),
                "a hit must not consume the entry"
            );
            assert_eq!(cache.len(), 1);
        }

        #[test]
        fn re_inserting_a_key_replaces_rather_than_grows() {
            let cache = DecisionCache::default();
            let key = DecisionKey::new(&subject(&["steward"], false), MetadataOperation::ViewBasic);

            cache.insert(key.clone(), AccessPredicate::Nothing);
            cache.insert(key.clone(), AccessPredicate::All);

            assert_eq!(cache.len(), 1);
            assert_eq!(cache.get(&key), Some(AccessPredicate::All));
        }
    }

    mod the_bound_is_real {
        use super::*;

        fn key(n: usize) -> DecisionKey {
            DecisionKey::new(
                &subject(&[&format!("role-{n}")], false),
                MetadataOperation::ViewBasic,
            )
        }

        /// Unbounded is the failure this exists to prevent: an authorization
        /// cache that grows with traffic is a memory leak on the request path.
        #[test]
        fn the_cache_never_exceeds_its_capacity() {
            let cache = DecisionCache::with_capacity(4);

            for n in 0..100 {
                cache.insert(key(n), AccessPredicate::All);
            }

            assert_eq!(cache.len(), 4);
        }

        /// Least-recently-used, not arbitrary. An eviction policy that dropped
        /// the *hot* entry would leave the cache full and useless — every
        /// request a miss, and the memory still spent.
        #[test]
        fn the_coldest_entry_is_evicted_and_the_hot_one_survives() {
            let cache = DecisionCache::with_capacity(2);
            cache.insert(key(1), AccessPredicate::All);
            cache.insert(key(2), AccessPredicate::Nothing);

            // Touch 1, making 2 the coldest.
            assert!(cache.get(&key(1)).is_some());
            cache.insert(key(3), AccessPredicate::All);

            assert_eq!(cache.get(&key(1)), Some(AccessPredicate::All), "still hot");
            assert_eq!(cache.get(&key(2)), None, "coldest, evicted");
            assert_eq!(cache.get(&key(3)), Some(AccessPredicate::All), "newest");
        }

        /// Recency must come from *insertion* too, not only from reads. With
        /// eviction driven by a counter that never advances on insert, every
        /// entry looks equally cold and the victim is whatever the hash map
        /// happens to yield first — which is not an eviction policy, it is a
        /// coin toss that occasionally throws away the entry just added.
        #[test]
        fn with_no_reads_at_all_the_oldest_insertion_is_the_one_evicted() {
            let cache = DecisionCache::with_capacity(2);

            cache.insert(key(1), AccessPredicate::All);
            cache.insert(key(2), AccessPredicate::All);
            cache.insert(key(3), AccessPredicate::All);

            assert_eq!(cache.get(&key(1)), None, "inserted first, evicted first");
            assert!(cache.get(&key(2)).is_some());
            assert!(cache.get(&key(3)).is_some());
        }

        #[test]
        fn a_capacity_of_one_still_works() {
            let cache = DecisionCache::with_capacity(1);
            cache.insert(key(1), AccessPredicate::All);
            cache.insert(key(2), AccessPredicate::Nothing);

            assert_eq!(cache.len(), 1);
            assert_eq!(cache.get(&key(2)), Some(AccessPredicate::Nothing));
        }
    }
}
