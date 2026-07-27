//! Policy evaluation: `(principal, operation, resource) -> Decision`.
//!
//! Pure. No I/O, no database, no clock. Fetching policies is the facade's job,
//! which is what makes every rule here exhaustively testable — and a surviving
//! mutant in this crate is a security bug, not a style issue.

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
}

impl ResourceMatcher {
    #[must_use]
    pub fn matches(&self, resource: &Resource) -> bool {
        match self {
            ResourceMatcher::All => true,
            ResourceMatcher::FqnPrefix(prefix) => resource.fqn.starts_with(prefix.as_str()),
            ResourceMatcher::Tagged(tag) => resource.tags.iter().any(|t| t == tag),
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
        match (rule.effect, &rule.resources) {
            (Effect::Allow, ResourceMatcher::All) => allow_all = true,
            (Effect::Deny, ResourceMatcher::All) => deny_all = true,
            (Effect::Allow, ResourceMatcher::FqnPrefix(p)) => allow_prefixes.push(p.clone()),
            (Effect::Deny, ResourceMatcher::FqnPrefix(p)) => deny_prefixes.push(p.clone()),
            // Tag matching needs the resource's tags, which a row-level
            // predicate does not carry until Epic 25 puts them in the query.
            // Compiling it as unrestricted would be a leak, so it contributes
            // nothing and the request falls back to whatever else allows it.
            (_, ResourceMatcher::Tagged(_)) => {}
        }
    }

    if deny_all {
        return AccessPredicate::Nothing;
    }
    if allow_all {
        return if deny_prefixes.is_empty() {
            AccessPredicate::All
        } else {
            AccessPredicate::Fqn {
                // An empty prefix matches every FQN.
                allow_prefixes: vec![String::new()],
                deny_prefixes,
            }
        };
    }
    if allow_prefixes.is_empty() {
        return AccessPredicate::Nothing;
    }
    AccessPredicate::Fqn {
        allow_prefixes,
        deny_prefixes,
    }
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
