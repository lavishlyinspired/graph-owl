//! OWL 2 RL forward-chaining overlay (pure, no I/O) — Epic 6.
//!
//! Eight axioms as **built-in functions** rather than a rule interpreter over an
//! OWL encoding (`06-engine-reasoning.md` Slice A). Eight things that can be
//! read and tested beat an interpreter plus an encoding nobody can debug.
//!
//! Evaluation is **semi-naive** and **budgeted**: each iteration joins against
//! the previous iteration's output rather than the whole graph, and four limits
//! stop a run that would otherwise not finish. A stopped run returns what it
//! had along with the reason it stopped — capping is a result, not an error.

use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

fn v(ns: u16, id: &str) -> Sid {
    Sid::new(ns, id)
}

fn rdf_type() -> Sid {
    v(namespace::RDF, "type")
}

/// The graph derived facts are written to.
///
/// Never the default graph: a run replaces this graph wholesale, so derivations
/// landing beside assertions would make the next run delete asserted data.
#[must_use]
pub fn reasoning_graph() -> Sid {
    Sid::dsc("graph:reasoning")
}

/// Which axiom drew a conclusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleName {
    SubClassOf,
    SubPropertyOf,
    Transitive,
    Symmetric,
    InverseOf,
    Domain,
    Range,
    SameAs,
}

impl RuleName {
    /// The eight, in the order they run. Order does not change the fixpoint —
    /// that is the point of iterating — but a fixed order makes a run
    /// reproducible, which is what makes a derivation reviewable.
    pub const ALL: [Self; 8] = [
        Self::SubClassOf,
        Self::SubPropertyOf,
        Self::Transitive,
        Self::Symmetric,
        Self::InverseOf,
        Self::Domain,
        Self::Range,
        Self::SameAs,
    ];

    /// The rule's identity as a subject, for provenance written to the graph.
    #[must_use]
    pub fn sid(self) -> Sid {
        Sid::dsc(match self {
            Self::SubClassOf => "rule:subClassOf",
            Self::SubPropertyOf => "rule:subPropertyOf",
            Self::Transitive => "rule:transitive",
            Self::Symmetric => "rule:symmetric",
            Self::InverseOf => "rule:inverseOf",
            Self::Domain => "rule:domain",
            Self::Range => "rule:range",
            Self::SameAs => "rule:sameAs",
        })
    }
}

/// One route to a conclusion: the rule that fired and what it fired on.
#[derive(Debug, Clone, PartialEq)]
pub struct Derivation {
    pub rule: RuleName,
    pub premises: Vec<Flake>,
}

/// A conclusion, with every route that reaches it.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedFact {
    /// `cx` is always [`reasoning_graph`].
    pub fact: Flake,
    /// At least one, and more when the same fact follows several ways.
    pub derivations: Vec<Derivation>,
    /// The **minimum** confidence along the best available route.
    pub confidence: f64,
}

/// Why a run stopped early.
///
/// Four reasons rather than a boolean because they demand opposite responses:
/// the iteration cap means the rule set has a cycle to fix, the fact cap means
/// the graph outgrew the budget, and an operator told only "capped" can act on
/// neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CappedReason {
    Duration,
    Facts,
    Iterations,
    Memory,
}

/// What one invocation is allowed to spend, and what it is allowed to read.
#[derive(Debug, Clone, PartialEq)]
pub struct Budget {
    pub max_duration: Duration,
    pub max_facts: usize,
    pub max_iterations: usize,
    /// Accounted against the working fact set, never sampled from the process.
    /// A process reading includes everything else in the binary and moves under
    /// an allocator this crate does not control, so the same input would give a
    /// different answer on each run and the limit would not be testable.
    pub max_memory_bytes: usize,
    /// The rules permitted to fire. Removing one removes its derivations.
    pub rules: Vec<RuleName>,
    /// Named graphs whose facts may feed inference. The default graph always
    /// does; everything else is opt-in, because reasoning over an unconfirmed
    /// extraction launders a guess into something that looks like catalog
    /// truth.
    pub include_graphs: Vec<Sid>,
    /// Confidence carried by a fact from an included named graph. The default
    /// graph is asserted truth and carries `1.0`; an extraction graph carries
    /// whatever the extractor claimed.
    ///
    /// One number rather than a map because there is exactly one such graph
    /// today. A map is the change to make when there are two.
    pub named_graph_confidence: f64,
}

impl Default for Budget {
    /// The defaults in `06-engine-reasoning.md`, each sized against the 1M-flake
    /// catalog `00a-product-position.md` commits to:
    ///
    /// - **30s** — the longest a synchronous run can take before the caller
    ///   should have been given a job id instead.
    /// - **`100_000` facts** — a tenth of the target catalog. A run inferring
    ///   more than that has found a modelling error, not a fact.
    /// - **20 iterations** — deeper than any hierarchy this models. Reaching it
    ///   means the rule set cycles.
    /// - **512MB** — the working set a single request may hold without
    ///   threatening a server sized for concurrent queries.
    fn default() -> Self {
        Self {
            max_duration: Duration::from_secs(30),
            max_facts: 100_000,
            max_iterations: 20,
            max_memory_bytes: 512 * 1024 * 1024,
            rules: RuleName::ALL.to_vec(),
            include_graphs: Vec::new(),
            named_graph_confidence: 1.0,
        }
    }
}

/// Everything one run concluded, and whether it finished.
#[derive(Debug, Clone, PartialEq)]
pub struct Reasoning {
    pub facts: Vec<DerivedFact>,
    /// `None` means fixpoint. It is the only signal of completeness.
    pub capped: Option<CappedReason>,
    pub iterations: usize,
    pub duration: Duration,
    /// Premise pairs examined. Semi-naive and naive evaluation produce the same
    /// answer and wildly different counts, so this is the only way a test can
    /// tell them apart.
    pub joins: u64,
    /// The working set this run accounted itself at, against
    /// [`Budget::max_memory_bytes`]. Reported because a limit whose accounting
    /// nobody can inspect is a field rather than a limit.
    pub accounted_bytes: usize,
}

/// A fact's identity for deduplication: `(s, p, o)`.
///
/// `cx` is deliberately excluded even though the plan's dedup key names it — a
/// conclusion that restates an asserted fact is not news just because it would
/// land in a different graph, and including `cx` would make every run re-derive
/// the whole base.
///
/// The object is rendered rather than hashed because `FlakeValue` carries a
/// float and is therefore not `Eq`/`Hash`: a NaN is not equal to itself.
/// Rendering sidesteps that without asserting an equality the type declines to
/// offer.
type Key = (Sid, Sid, String);

fn key(f: &Flake) -> Key {
    (f.s.clone(), f.p.clone(), format!("{:?}", f.o))
}

/// The object as a reference, or `None` for a literal.
///
/// Every one of the eight axioms relates *entities*. A literal object cannot be
/// a class, a property or an identity, so a rule reading one would be reasoning
/// about something that cannot participate.
fn obj(flake: &Flake) -> Option<&Sid> {
    match &flake.o {
        FlakeValue::Ref(sid) => Some(sid),
        _ => None,
    }
}

/// Flakes stating `p` — asserted only.
fn with_predicate<'a>(facts: &'a [Flake], p: &Sid) -> impl Iterator<Item = &'a Flake> {
    let p = p.clone();
    facts.iter().filter(move |f| f.op && f.p == p)
}

/// Subjects declared to be of type `class`.
fn typed_as(facts: &[Flake], class: &Sid) -> HashSet<Sid> {
    with_predicate(facts, &rdf_type())
        .filter(|f| obj(f) == Some(class))
        .map(|f| f.s.clone())
        .collect()
}

/// A conclusion, stamped so it cannot appear older than what produced it.
///
/// `t` is the **maximum** of the premises. A derived fact carrying an earlier
/// `t` would be visible at an instant before the facts that imply it, which
/// would make time travel and reasoning disagree about the same moment.
fn conclude(s: Sid, p: Sid, o: Sid, premises: &[&Flake]) -> Flake {
    Flake {
        s,
        p,
        o: FlakeValue::Ref(o),
        cx: Some(reasoning_graph()),
        t: premises.iter().map(|f| f.t).max().unwrap_or(0),
        op: true,
    }
}

/// Does this flake state something about the *schema* rather than the data?
///
/// The distinction is what makes semi-naive evaluation both fast and complete.
/// A new data fact can only imply conclusions that join it to what is already
/// known, so restricting the join to the delta loses nothing. A new **axiom**
/// changes what every *existing* fact implies, and an iteration that introduces
/// one has to join against everything or it silently drops conclusions — the
/// completeness hole a naive reading of semi-naive evaluation leaves.
///
/// Axioms are a tiny fraction of a real graph and are rarely derived, so the
/// full join almost never runs.
fn is_axiom(f: &Flake) -> bool {
    let schema = [
        (namespace::RDFS, "subClassOf"),
        (namespace::RDFS, "subPropertyOf"),
        (namespace::RDFS, "domain"),
        (namespace::RDFS, "range"),
        (namespace::OWL, "inverseOf"),
        // `sameAs` is here because it behaves like one: it licenses copying
        // every property an entity already had, so a new identity statement
        // reaches arbitrarily far back into old data.
        (namespace::OWL, "sameAs"),
    ];
    if schema.iter().any(|(ns, id)| f.p == v(*ns, id)) {
        return true;
    }
    // A property characteristic is stated as a type, and gives the same reach.
    f.p == rdf_type()
        && matches!(
            obj(f),
            Some(o) if *o == v(namespace::OWL, "TransitiveProperty")
                || *o == v(namespace::OWL, "SymmetricProperty")
        )
}

/// The bytes one fact and its provenance are accounted at.
///
/// Deliberately an over-estimate: the purpose is to refuse before exhaustion,
/// not to report a precise number, and under-estimating defeats the limit.
fn footprint(fact: &DerivedFact) -> usize {
    let flake = size_of::<Flake>() + 64;
    flake
        + fact
            .derivations
            .iter()
            .map(|d| size_of::<Derivation>() + d.premises.len() * flake)
            .sum::<usize>()
}

/// One iteration's working state.
struct Pass<'a> {
    /// Everything known at the start of this iteration, including the delta.
    all: &'a [Flake],
    /// Facts new since the previous iteration. Every conclusion draws at least
    /// one premise from here.
    new: &'a [Flake],
    /// This iteration must join against everything — the first pass, or one
    /// whose delta carries an axiom.
    naive: bool,
    joins: u64,
    out: Vec<(Flake, RuleName, Vec<Flake>)>,
}

impl Pass<'_> {
    fn emit(&mut self, s: Sid, p: Sid, o: Sid, rule: RuleName, premises: &[&Flake]) {
        let fact = conclude(s, p, o, premises);
        let premises = premises.iter().map(|f| (*f).clone()).collect();
        self.out.push((fact, rule, premises));
    }
}

fn rule_sub_class_of(pass: &mut Pass) {
    // rdfs:subClassOf — (a type C1), (C1 subClassOf C2) => (a type C2)
    let (all, new) = (pass.all, pass.new);
    for axiom in with_predicate(all, &v(namespace::RDFS, "subClassOf")) {
        let Some(super_class) = obj(axiom) else {
            continue;
        };
        for member in with_predicate(new, &rdf_type()) {
            pass.joins += 1;
            if obj(member) != Some(&axiom.s) {
                continue;
            }
            pass.emit(
                member.s.clone(),
                rdf_type(),
                super_class.clone(),
                RuleName::SubClassOf,
                &[axiom, member],
            );
        }
    }
}

fn rule_sub_property_of(pass: &mut Pass) {
    // rdfs:subPropertyOf — (a p1 b), (p1 subPropertyOf p2) => (a p2 b)
    let (all, new) = (pass.all, pass.new);
    for axiom in with_predicate(all, &v(namespace::RDFS, "subPropertyOf")) {
        let Some(super_property) = obj(axiom) else {
            continue;
        };
        for used in with_predicate(new, &axiom.s) {
            pass.joins += 1;
            let Some(object) = obj(used) else { continue };
            pass.emit(
                used.s.clone(),
                super_property.clone(),
                object.clone(),
                RuleName::SubPropertyOf,
                &[axiom, used],
            );
        }
    }
}

fn rule_transitive(pass: &mut Pass) {
    // owl:TransitiveProperty — (a p b), (b p c) => (a p c)
    //
    // Two data premises, so semi-naive evaluation needs both orders: a new edge
    // can be either half of the composition, and running only one order drops
    // every conclusion where the new edge is the other half.
    let (all, new, naive) = (pass.all, pass.new, pass.naive);
    for property in typed_as(all, &v(namespace::OWL, "TransitiveProperty")) {
        let old: Vec<&Flake> = with_predicate(all, &property).collect();
        let fresh: Vec<&Flake> = with_predicate(new, &property).collect();
        let orders: &[(&Vec<&Flake>, &Vec<&Flake>)] = if naive {
            &[(&old, &old)]
        } else {
            &[(&fresh, &old), (&old, &fresh)]
        };
        for (lefts, rights) in orders {
            for left in *lefts {
                let Some(mid) = obj(left) else { continue };
                for right in *rights {
                    pass.joins += 1;
                    if &right.s != mid {
                        continue;
                    }
                    let Some(end) = obj(right) else { continue };
                    pass.emit(
                        left.s.clone(),
                        property.clone(),
                        end.clone(),
                        RuleName::Transitive,
                        &[left, right],
                    );
                }
            }
        }
    }
}

fn rule_symmetric(pass: &mut Pass) {
    // owl:SymmetricProperty — (a p b) => (b p a)
    let (all, new) = (pass.all, pass.new);
    for property in typed_as(all, &v(namespace::OWL, "SymmetricProperty")) {
        for edge in with_predicate(new, &property) {
            pass.joins += 1;
            let Some(object) = obj(edge) else { continue };
            pass.emit(
                object.clone(),
                property.clone(),
                edge.s.clone(),
                RuleName::Symmetric,
                &[edge],
            );
        }
    }
}

fn rule_inverse_of(pass: &mut Pass) {
    // owl:inverseOf — (a p1 b) => (b p2 a), read in **both** directions of the
    // axiom: `p inverseOf q` states `q inverseOf p` just as strongly.
    let (all, new) = (pass.all, pass.new);
    for axiom in with_predicate(all, &v(namespace::OWL, "inverseOf")) {
        let Some(right) = obj(axiom) else { continue };
        for (from, to) in [(&axiom.s, right), (right, &axiom.s)] {
            for edge in with_predicate(new, from) {
                pass.joins += 1;
                let Some(object) = obj(edge) else { continue };
                pass.emit(
                    object.clone(),
                    to.clone(),
                    edge.s.clone(),
                    RuleName::InverseOf,
                    &[axiom, edge],
                );
            }
        }
    }
}

fn rule_domain(pass: &mut Pass) {
    // rdfs:domain — (a p b), (p domain C) => (a type C). Subject side.
    let (all, new) = (pass.all, pass.new);
    for axiom in with_predicate(all, &v(namespace::RDFS, "domain")) {
        let Some(class) = obj(axiom) else { continue };
        for used in with_predicate(new, &axiom.s) {
            pass.joins += 1;
            pass.emit(
                used.s.clone(),
                rdf_type(),
                class.clone(),
                RuleName::Domain,
                &[axiom, used],
            );
        }
    }
}

fn rule_range(pass: &mut Pass) {
    // rdfs:range — (a p b), (p range C) => (b type C). Object side.
    let (all, new) = (pass.all, pass.new);
    for axiom in with_predicate(all, &v(namespace::RDFS, "range")) {
        let Some(class) = obj(axiom) else { continue };
        for used in with_predicate(new, &axiom.s) {
            pass.joins += 1;
            let Some(object) = obj(used) else { continue };
            pass.emit(
                object.clone(),
                rdf_type(),
                class.clone(),
                RuleName::Range,
                &[axiom, used],
            );
        }
    }
}

fn rule_same_as(pass: &mut Pass) {
    // owl:sameAs — (a sameAs b), (a p o) => (b p o), in both directions,
    // because identity that depended on assertion order would not be identity.
    let (all, new) = (pass.all, pass.new);
    for axiom in with_predicate(all, &v(namespace::OWL, "sameAs")) {
        let Some(right) = obj(axiom) else { continue };
        for (from, to) in [(&axiom.s, right), (right, &axiom.s)] {
            for held in new.iter().filter(|f| f.op) {
                pass.joins += 1;
                if &held.s != from {
                    continue;
                }
                let Some(object) = obj(held) else { continue };
                pass.emit(
                    to.clone(),
                    held.p.clone(),
                    object.clone(),
                    RuleName::SameAs,
                    &[axiom, held],
                );
            }
        }
    }
}

fn run(rule: RuleName) -> fn(&mut Pass) {
    match rule {
        RuleName::SubClassOf => rule_sub_class_of,
        RuleName::SubPropertyOf => rule_sub_property_of,
        RuleName::Transitive => rule_transitive,
        RuleName::Symmetric => rule_symmetric,
        RuleName::InverseOf => rule_inverse_of,
        RuleName::Domain => rule_domain,
        RuleName::Range => rule_range,
        RuleName::SameAs => rule_same_as,
    }
}

/// Everything the facts imply but do not state, under the default budget.
#[must_use]
pub fn derive(facts: &[Flake]) -> Reasoning {
    derive_within(facts, &Budget::default())
}

/// Everything the facts imply but do not state, bounded.
///
/// Iterates because one pass reaches depth one: `C1 ⊑ C2 ⊑ C3` needs the first
/// conclusion in hand before the second can be drawn. Deduplication against
/// everything already known is what makes it **terminate** — a symmetric
/// property otherwise re-derives its own reverse forever.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn derive_within(facts: &[Flake], budget: &Budget) -> Reasoning {
    let started = Instant::now();

    // Only the default graph and the graphs this run was told to include.
    // Filtering the input once is what keeps every rule from having to
    // re-decide the question.
    let readable: Vec<Flake> = facts
        .iter()
        .filter(|f| match &f.cx {
            None => true,
            Some(graph) => budget.include_graphs.contains(graph),
        })
        .cloned()
        .collect();

    let mut confidence: HashMap<Key, f64> = readable
        .iter()
        .map(|f| {
            let c = if f.cx.is_none() {
                1.0
            } else {
                budget.named_graph_confidence
            };
            (key(f), c)
        })
        .collect();

    let mut known: HashSet<Key> = readable.iter().filter(|f| f.op).map(key).collect();
    let mut all = readable.clone();
    let mut derived: Vec<DerivedFact> = Vec::new();
    let mut index: HashMap<Key, usize> = HashMap::new();
    let mut delta = readable;
    let mut naive = true;
    let mut joins = 0_u64;
    let mut accounted = 0_usize;
    let mut iterations = 0_usize;

    let capped = loop {
        if started.elapsed() >= budget.max_duration {
            break Some(CappedReason::Duration);
        }
        if accounted >= budget.max_memory_bytes {
            break Some(CappedReason::Memory);
        }
        if iterations >= budget.max_iterations {
            break Some(CappedReason::Iterations);
        }
        iterations += 1;

        let mut pass = Pass {
            all: &all,
            new: &delta,
            naive,
            joins: 0,
            out: Vec::new(),
        };
        for rule in &budget.rules {
            run(*rule)(&mut pass);
        }
        joins += pass.joins;

        let mut fresh: Vec<Flake> = Vec::new();
        let mut over_budget = false;
        for (fact, rule, premises) in pass.out {
            let k = key(&fact);
            // A route to a fact already concluded is not a new fact, but it is
            // a new explanation — and an explanation dropped because of arrival
            // order is one a reader would have wanted.
            if let Some(position) = index.get(&k) {
                let existing = &mut derived[*position];
                let route = Derivation { rule, premises };
                if !existing.derivations.contains(&route) {
                    let strength = weakest(&route.premises, &confidence);
                    existing.derivations.push(route);
                    // A fact provable two ways is as certain as its best route.
                    existing.confidence = existing.confidence.max(strength);
                    confidence.insert(k, existing.confidence);
                }
                continue;
            }
            if !known.insert(k.clone()) {
                continue;
            }
            if derived.len() >= budget.max_facts {
                over_budget = true;
                break;
            }
            let strength = weakest(&premises, &confidence);
            confidence.insert(k.clone(), strength);
            let entry = DerivedFact {
                fact: fact.clone(),
                derivations: vec![Derivation { rule, premises }],
                confidence: strength,
            };
            accounted += footprint(&entry);
            index.insert(k, derived.len());
            derived.push(entry);
            fresh.push(fact);
        }

        if over_budget {
            break Some(CappedReason::Facts);
        }
        if fresh.is_empty() {
            break None;
        }
        // A delta carrying an axiom changes what every *existing* fact implies,
        // so the next iteration joins against everything. Widening the delta
        // rather than setting a flag each rule must remember to honour: the
        // flag version shipped, and it reached only `rule_transitive` — the
        // other seven kept scanning the delta and silently dropped every
        // conclusion that needed a new axiom and an old fact.
        naive = fresh.iter().any(is_axiom);
        all.extend(fresh.iter().cloned());
        delta = if naive { all.clone() } else { fresh };
    };

    Reasoning {
        facts: derived,
        capped,
        iterations,
        duration: started.elapsed(),
        joins,
        accounted_bytes: accounted,
    }
}

/// The least certain premise.
///
/// **Minimum, not product.** Reasoning does not compound uncertainty the way
/// independent sources do: a conclusion drawn from two facts an extractor was
/// 90% sure of is 90% certain, because it is exactly as good as the weaker of
/// the two. Under the product rule a depth-4 derivation from good premises
/// scores 0.66 and looks worthless, which is how explainable inference gets
/// switched off.
fn weakest(premises: &[Flake], confidence: &HashMap<Key, f64>) -> f64 {
    premises
        .iter()
        .map(|p| confidence.get(&key(p)).copied().unwrap_or(1.0))
        .fold(1.0_f64, f64::min)
}

/// Why a fact holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Explanation {
    /// Somebody stated it. The chain ends here.
    Asserted(Flake),
    /// Every route that reaches it.
    Derived { chains: Vec<Chain> },
    /// A premise already being explained further up this chain. Only reachable
    /// through a cyclic ontology, and naming it beats truncating in silence.
    Circular(Flake),
    /// Neither stated nor implied. The HTTP surface turns this into a `404`.
    Unknown,
}

/// One route, with each premise explained in turn.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    pub rule: RuleName,
    pub premises: Vec<Explanation>,
}

/// Why `target` holds, all the way down to assertions.
///
/// Recursive rather than one level deep: an explanation naming a derived
/// premise and stopping tells the reader nothing about why *that* held, and
/// that is the interesting half.
#[must_use]
pub fn explain(reasoning: &Reasoning, asserted: &[Flake], target: &Flake) -> Explanation {
    let index: HashMap<Key, &DerivedFact> =
        reasoning.facts.iter().map(|d| (key(&d.fact), d)).collect();
    let stated: HashMap<Key, &Flake> = asserted
        .iter()
        .filter(|f| f.op)
        .map(|f| (key(f), f))
        .collect();
    let mut open = HashSet::new();
    walk(target, &index, &stated, &mut open)
}

fn walk(
    target: &Flake,
    index: &HashMap<Key, &DerivedFact>,
    stated: &HashMap<Key, &Flake>,
    open: &mut HashSet<Key>,
) -> Explanation {
    let k = key(target);
    if let Some(fact) = stated.get(&k) {
        return Explanation::Asserted((*fact).clone());
    }
    if !open.insert(k.clone()) {
        return Explanation::Circular(target.clone());
    }
    let explanation = match index.get(&k) {
        None => Explanation::Unknown,
        Some(derived) => Explanation::Derived {
            chains: derived
                .derivations
                .iter()
                .map(|d| Chain {
                    rule: d.rule,
                    premises: d
                        .premises
                        .iter()
                        .map(|p| walk(p, index, stated, open))
                        .collect(),
                })
                .collect(),
        },
    };
    open.remove(&k);
    explanation
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};

    fn sid(ns: u16, id: &str) -> Sid {
        Sid::new(ns, id)
    }

    /// An asserted fact at t=1, default graph.
    fn f(s: Sid, p: Sid, o: Sid) -> Flake {
        Flake {
            s,
            p,
            o: FlakeValue::Ref(o),
            cx: None,
            t: 1,
            op: true,
        }
    }

    fn a(id: &str) -> Sid {
        sid(namespace::DSC, id)
    }
    fn rdf_type() -> Sid {
        sid(namespace::RDF, "type")
    }
    fn sub_class_of() -> Sid {
        sid(namespace::RDFS, "subClassOf")
    }
    fn sub_property_of() -> Sid {
        sid(namespace::RDFS, "subPropertyOf")
    }
    fn domain() -> Sid {
        sid(namespace::RDFS, "domain")
    }
    fn range() -> Sid {
        sid(namespace::RDFS, "range")
    }
    fn transitive() -> Sid {
        sid(namespace::OWL, "TransitiveProperty")
    }
    fn symmetric() -> Sid {
        sid(namespace::OWL, "SymmetricProperty")
    }
    fn inverse_of() -> Sid {
        sid(namespace::OWL, "inverseOf")
    }
    fn same_as() -> Sid {
        sid(namespace::OWL, "sameAs")
    }

    /// Did `derive` produce this exact triple?
    fn derived(facts: &[Flake], s: Sid, p: Sid, o: Sid) -> bool {
        let want = (s, p, FlakeValue::Ref(o));
        derive(facts)
            .facts
            .iter()
            .any(|d| (d.fact.s.clone(), d.fact.p.clone(), d.fact.o.clone()) == want)
    }

    mod subsumption {
        use super::*;

        #[test]
        fn a_type_flows_up_three_levels_of_sub_class() {
            // Depth 3 is the specification, not depth 1: a single-step
            // implementation passes the shallow case and fails here.
            let facts = vec![
                f(a("thing"), rdf_type(), a("C1")),
                f(a("C1"), sub_class_of(), a("C2")),
                f(a("C2"), sub_class_of(), a("C3")),
            ];

            assert!(derived(&facts, a("thing"), rdf_type(), a("C3")));
        }

        #[test]
        fn nothing_flows_up_without_the_sub_class_axiom() {
            let facts = vec![f(a("thing"), rdf_type(), a("C1"))];

            assert!(!derived(&facts, a("thing"), rdf_type(), a("C2")));
        }

        #[test]
        fn a_predicate_flows_up_its_sub_property_chain() {
            let facts = vec![
                f(a("x"), a("p1"), a("y")),
                f(a("p1"), sub_property_of(), a("p2")),
                f(a("p2"), sub_property_of(), a("p3")),
            ];

            assert!(derived(&facts, a("x"), a("p3"), a("y")));
        }
    }

    mod property_characteristics {
        use super::*;

        #[test]
        fn a_transitive_property_reaches_the_far_end_of_a_three_hop_chain() {
            let facts = vec![
                f(a("partOf"), rdf_type(), transitive()),
                f(a("a"), a("partOf"), a("b")),
                f(a("b"), a("partOf"), a("c")),
                f(a("c"), a("partOf"), a("d")),
            ];

            assert!(derived(&facts, a("a"), a("partOf"), a("d")));
        }

        #[test]
        fn a_property_not_declared_transitive_does_not_compose() {
            let facts = vec![
                f(a("likes"), rdf_type(), transitive()),
                f(a("a"), a("knows"), a("b")),
                f(a("b"), a("knows"), a("c")),
            ];

            // The declaration is present but names a *different* predicate. A
            // rule that fired on any transitive declaration anywhere would pass
            // this by accident.
            assert!(!derived(&facts, a("a"), a("knows"), a("c")));
        }

        #[test]
        fn two_disconnected_edges_do_not_compose_just_because_the_property_is_transitive() {
            // Transitivity joins on the *midpoint*. Without asserting a
            // non-composition, an implementation that joins on "any other edge"
            // still produces the far edge of a real chain and looks correct.
            let facts = vec![
                f(a("partOf"), rdf_type(), transitive()),
                f(a("a"), a("partOf"), a("b")),
                f(a("c"), a("partOf"), a("d")),
            ];

            assert!(derive(&facts).facts.is_empty());
        }

        #[test]
        fn a_symmetric_property_derives_its_reverse() {
            let facts = vec![
                f(a("marriedTo"), rdf_type(), symmetric()),
                f(a("x"), a("marriedTo"), a("y")),
            ];

            assert!(derived(&facts, a("y"), a("marriedTo"), a("x")));
        }

        #[test]
        fn a_property_not_declared_symmetric_does_not_reverse() {
            let facts = vec![f(a("parentOf"), a("x"), a("y"))];

            assert!(!derived(&facts, a("y"), a("parentOf"), a("x")));
        }

        #[test]
        fn an_inverse_property_derives_the_opposite_direction() {
            let facts = vec![
                f(a("hasParent"), inverse_of(), a("hasChild")),
                f(a("kid"), a("hasParent"), a("adult")),
            ];

            assert!(derived(&facts, a("adult"), a("hasChild"), a("kid")));
        }

        #[test]
        fn inverse_is_read_in_both_directions_of_the_axiom() {
            // `p inverseOf q` also means `q inverseOf p`. Implementing only one
            // direction silently halves the rule.
            let facts = vec![
                f(a("hasParent"), inverse_of(), a("hasChild")),
                f(a("adult"), a("hasChild"), a("kid")),
            ];

            assert!(derived(&facts, a("kid"), a("hasParent"), a("adult")));
        }
    }

    mod domain_and_range {
        use super::*;

        #[test]
        fn domain_types_the_subject() {
            let facts = vec![
                f(a("worksAt"), domain(), a("Person")),
                f(a("alice"), a("worksAt"), a("acme")),
            ];

            assert!(derived(&facts, a("alice"), rdf_type(), a("Person")));
        }

        #[test]
        fn domain_does_not_type_the_object() {
            // The classic bug is a swap. Asserting only the positive case lets
            // an implementation that types the wrong side pass.
            let facts = vec![
                f(a("worksAt"), domain(), a("Person")),
                f(a("alice"), a("worksAt"), a("acme")),
            ];

            assert!(!derived(&facts, a("acme"), rdf_type(), a("Person")));
        }

        #[test]
        fn range_types_the_object() {
            let facts = vec![
                f(a("worksAt"), range(), a("Company")),
                f(a("alice"), a("worksAt"), a("acme")),
            ];

            assert!(derived(&facts, a("acme"), rdf_type(), a("Company")));
        }

        #[test]
        fn range_does_not_type_the_subject() {
            let facts = vec![
                f(a("worksAt"), range(), a("Company")),
                f(a("alice"), a("worksAt"), a("acme")),
            ];

            assert!(!derived(&facts, a("alice"), rdf_type(), a("Company")));
        }
    }

    mod identity {
        use super::*;

        #[test]
        fn same_as_copies_properties_forward() {
            let facts = vec![
                f(a("x"), same_as(), a("y")),
                f(a("x"), a("worksAt"), a("acme")),
            ];

            assert!(derived(&facts, a("y"), a("worksAt"), a("acme")));
        }

        #[test]
        fn same_as_copies_only_from_the_identified_pair() {
            // A third entity's properties are nobody else's. Without this, a
            // condition that widened to "any asserted fact" would copy the
            // whole graph onto both sides of every identity.
            let facts = vec![
                f(a("x"), same_as(), a("y")),
                f(a("z"), a("worksAt"), a("acme")),
            ];

            assert!(!derived(&facts, a("x"), a("worksAt"), a("acme")));
            assert!(!derived(&facts, a("y"), a("worksAt"), a("acme")));
        }

        #[test]
        fn same_as_does_not_copy_a_retracted_property() {
            let mut withdrawn = f(a("x"), a("worksAt"), a("acme"));
            withdrawn.op = false;
            let facts = vec![f(a("x"), same_as(), a("y")), withdrawn];

            assert!(!derived(&facts, a("y"), a("worksAt"), a("acme")));
        }

        #[test]
        fn same_as_copies_properties_backward() {
            // `sameAs` is symmetric. Copying only left-to-right makes identity
            // depend on which way the assertion happened to be written.
            let facts = vec![
                f(a("x"), same_as(), a("y")),
                f(a("y"), a("worksAt"), a("acme")),
            ];

            assert!(derived(&facts, a("x"), a("worksAt"), a("acme")));
        }
    }

    mod what_derivation_returns {
        use super::*;

        #[test]
        fn asserted_facts_are_not_returned_as_derivations() {
            // The result is what reasoning *added*. Echoing the input back would
            // make the overlay indistinguishable from the base — the thing
            // `00b` decision 14 exists to prevent.
            let facts = vec![
                f(a("thing"), rdf_type(), a("C1")),
                f(a("C1"), sub_class_of(), a("C2")),
            ];

            let out = derive(&facts);
            assert!(!out.facts.iter().any(|d| facts.contains(&d.fact)));
            assert!(!out.facts.is_empty());
        }

        #[test]
        fn a_derived_fact_never_predates_the_premises_that_produced_it() {
            let mut old = f(a("thing"), rdf_type(), a("C1"));
            old.t = 3;
            let mut newer = f(a("C1"), sub_class_of(), a("C2"));
            newer.t = 9;

            let out = derive(&[old, newer]);
            assert!(out.facts.iter().all(|d| d.fact.t == 9));
        }

        #[test]
        fn a_retraction_derives_nothing() {
            // `op = false` withdraws the fact. Reasoning over it would derive
            // conclusions from a premise the graph no longer states.
            let mut retracted = f(a("thing"), rdf_type(), a("C1"));
            retracted.op = false;
            let facts = vec![retracted, f(a("C1"), sub_class_of(), a("C2"))];

            assert!(derive(&facts).facts.is_empty());
        }

        #[test]
        fn deriving_twice_over_the_same_facts_produces_the_same_set() {
            let facts = vec![
                f(a("thing"), rdf_type(), a("C1")),
                f(a("C1"), sub_class_of(), a("C2")),
                f(a("marriedTo"), rdf_type(), symmetric()),
                f(a("x"), a("marriedTo"), a("y")),
            ];

            // The conclusions and the work, not the whole result: `duration` is
            // a clock reading rather than a conclusion, and asserting on it
            // would make this test fail for reasons that are not about
            // reasoning.
            assert_eq!(derive(&facts).facts, derive(&facts).facts);
            assert_eq!(derive(&facts).joins, derive(&facts).joins);
        }

        #[test]
        fn a_symmetric_property_terminates_rather_than_ping_ponging() {
            // (x p y) -> (y p x) -> (x p y) -> ... A run without deduplication
            // does not return.
            let facts = vec![
                f(a("marriedTo"), rdf_type(), symmetric()),
                f(a("x"), a("marriedTo"), a("y")),
            ];

            assert_eq!(derive(&facts).facts.len(), 1);
        }
    }

    /// # Slice B — the fixpoint terminates, deduplicates, and is semi-naive
    ///
    /// Slice A proved each axiom draws the right conclusion. These are the
    /// properties that make running it over real data safe rather than
    /// merely correct.
    mod the_fixpoint {
        use super::*;

        /// A cycle in the class hierarchy is a modelling error, not a crash.
        /// The bound is the visited set: without it this re-derives
        /// `thing type C1` forever.
        #[test]
        fn a_cyclic_class_hierarchy_terminates_and_derives_each_type_once() {
            let facts = vec![
                f(a("thing"), rdf_type(), a("C1")),
                f(a("C1"), sub_class_of(), a("C2")),
                f(a("C2"), sub_class_of(), a("C1")),
            ];

            let reasoning = derive(&facts);

            assert_eq!(
                reasoning.facts.len(),
                1,
                "only `thing type C2` is new; `type C1` was asserted: {:#?}",
                reasoning.facts
            );
            assert!(reasoning.capped.is_none(), "a cycle is not a cap");
        }

        /// Two routes to one conclusion is one fact carrying two derivations.
        /// Emitting it twice would double-count in every consumer; keeping one
        /// derivation would make the explanation incomplete.
        #[test]
        fn a_fact_derivable_two_ways_appears_once_with_two_derivations() {
            // `alice knows bob` follows both from `bob knows alice` under
            // symmetry and from `alice friendOf bob` under subPropertyOf.
            let facts = vec![
                f(a("knows"), rdf_type(), symmetric()),
                f(a("friendOf"), sub_property_of(), a("knows")),
                f(a("bob"), a("knows"), a("alice")),
                f(a("alice"), a("friendOf"), a("bob")),
            ];

            let reasoning = derive(&facts);
            let both = reasoning
                .facts
                .iter()
                .find(|d| d.fact.s == a("alice") && d.fact.p == a("knows"))
                .expect("alice knows bob should be derived");

            assert_eq!(both.derivations.len(), 2, "{:#?}", both.derivations);
            let rules: Vec<RuleName> = both.derivations.iter().map(|d| d.rule).collect();
            assert!(rules.contains(&RuleName::Symmetric), "{rules:?}");
            assert!(rules.contains(&RuleName::SubPropertyOf), "{rules:?}");
        }

        /// Reaching fixpoint is reported, and on a small graph it happens well
        /// inside the budget — otherwise the iteration cap would be doing the
        /// terminating, and nobody would notice a rule set that never settles.
        #[test]
        fn fixpoint_is_reached_well_inside_the_iteration_budget() {
            let facts = vec![
                f(a("thing"), rdf_type(), a("C1")),
                f(a("C1"), sub_class_of(), a("C2")),
                f(a("C2"), sub_class_of(), a("C3")),
            ];

            let reasoning = derive(&facts);

            assert!(
                reasoning.iterations >= 2,
                "depth 3 needs at least two passes"
            );
            assert!(
                reasoning.iterations < Budget::default().max_iterations,
                "{} iterations",
                reasoning.iterations
            );
            assert_eq!(reasoning.capped, None);
        }

        /// **Semi-naive, proved by counting.** The difference between joining
        /// each iteration against everything and joining it against only the
        /// previous iteration's output is invisible in the answer and enormous
        /// in the time — so the join count is reported and asserted.
        #[test]
        fn later_iterations_join_against_the_delta_not_the_whole_graph() {
            // A long chain: each iteration adds exactly one fact, so a naive
            // evaluator re-examines a growing set every pass and a semi-naive
            // one examines the single new fact.
            let mut facts = vec![f(a("thing"), rdf_type(), a("C0"))];
            for level in 0..12 {
                facts.push(f(
                    a(&format!("C{level}")),
                    sub_class_of(),
                    a(&format!("C{}", level + 1)),
                ));
            }

            let reasoning = derive(&facts);

            assert_eq!(reasoning.facts.len(), 12, "one type per level");
            // Naive evaluation examines every axiom against every type flake on
            // every pass: 12 axioms x a type set growing 1..12, over 12 passes.
            // Semi-naive examines each axiom against the one new type flake.
            let naive_would_be = 12 * 12 * 12;
            assert!(
                reasoning.joins < naive_would_be / 4,
                "{} joins is not semi-naive",
                reasoning.joins
            );
        }

        /// Nothing in, nothing out — and not an error. An empty graph is a
        /// legitimate state, not a failure to reason about.
        #[test]
        fn deriving_over_an_empty_fact_set_returns_empty() {
            let reasoning = derive(&[]);

            assert!(reasoning.facts.is_empty());
            assert_eq!(reasoning.capped, None);
            assert_eq!(reasoning.iterations, 1, "one pass finds nothing and stops");
        }

        /// A new *axiom* changes what every existing fact implies, so the
        /// iteration carrying one cannot restrict itself to the delta. This is
        /// the completeness hole a naive reading of semi-naive evaluation
        /// leaves: the conclusion below needs an old edge and a new axiom.
        #[test]
        fn an_axiom_derived_mid_run_still_fires_against_older_facts() {
            // `partOf` becomes transitive only once `type` flows up the class
            // hierarchy — by which point both its edges are old news.
            let facts = vec![
                f(a("partOf"), rdf_type(), a("MereologicalProperty")),
                f(a("MereologicalProperty"), sub_class_of(), transitive()),
                f(a("finger"), a("partOf"), a("hand")),
                f(a("hand"), a("partOf"), a("arm")),
            ];

            let reasoning = derive(&facts);

            assert!(
                reasoning.facts.iter().any(|d| d.fact.s == a("finger")
                    && d.fact.p == a("partOf")
                    && d.fact.o == FlakeValue::Ref(a("arm"))),
                "the transitivity axiom arrived in iteration 2: {:#?}",
                reasoning.facts
            );
        }
    }

    /// # Slice C — budgets bound the run
    ///
    /// Four limits, four reasons. Each test asserts the *matching* reason
    /// rather than merely that capping occurred: hitting the iteration cap
    /// means the rule set has a cycle to fix and hitting the fact cap means the
    /// graph outgrew the budget, and those demand opposite responses.
    mod budgets {
        use super::*;

        /// A chain long enough to outrun any budget set against it.
        fn long_chain(levels: usize) -> Vec<Flake> {
            let mut facts = vec![f(a("thing"), rdf_type(), a("C0"))];
            for level in 0..levels {
                facts.push(f(
                    a(&format!("C{level}")),
                    sub_class_of(),
                    a(&format!("C{}", level + 1)),
                ));
            }
            facts
        }

        #[test]
        fn the_fact_limit_stops_the_run_and_says_so() {
            let budget = Budget {
                max_facts: 5,
                ..Budget::default()
            };

            let reasoning = derive_within(&long_chain(40), &budget);

            assert_eq!(reasoning.capped, Some(CappedReason::Facts));
            assert!(
                reasoning.facts.len() <= 5,
                "the limit is a limit: {}",
                reasoning.facts.len()
            );
            assert!(!reasoning.facts.is_empty(), "a cap returns what it had");
        }

        #[test]
        fn the_iteration_limit_stops_the_run_and_says_so() {
            let budget = Budget {
                max_iterations: 3,
                ..Budget::default()
            };

            let reasoning = derive_within(&long_chain(40), &budget);

            assert_eq!(reasoning.capped, Some(CappedReason::Iterations));
            assert_eq!(reasoning.iterations, 3);
        }

        #[test]
        fn the_duration_limit_stops_the_run_and_says_so() {
            let budget = Budget {
                max_duration: std::time::Duration::ZERO,
                ..Budget::default()
            };

            let reasoning = derive_within(&long_chain(40), &budget);

            assert_eq!(reasoning.capped, Some(CappedReason::Duration));
        }

        #[test]
        fn the_memory_limit_stops_the_run_and_says_so() {
            let budget = Budget {
                max_memory_bytes: 1,
                ..Budget::default()
            };

            let reasoning = derive_within(&long_chain(40), &budget);

            assert_eq!(reasoning.capped, Some(CappedReason::Memory));
        }

        /// The completeness signal. `capped: None` is the *only* way a caller
        /// learns the answer is whole, which is why no other field may imply it.
        #[test]
        fn a_run_that_reaches_fixpoint_reports_no_cap() {
            let reasoning = derive_within(&long_chain(3), &Budget::default());

            assert_eq!(reasoning.capped, None);
            assert_eq!(reasoning.facts.len(), 3);
        }

        /// Capping is not an error. A partial answer with a stated reason is
        /// more useful than a failure, because the caller can act on both the
        /// facts and the reason.
        #[test]
        fn a_capped_run_returns_the_facts_it_had() {
            let budget = Budget {
                max_iterations: 2,
                ..Budget::default()
            };

            let reasoning = derive_within(&long_chain(40), &budget);

            assert!(
                !reasoning.facts.is_empty(),
                "a cap that discards its work is a failure with extra steps"
            );
        }
    }

    /// # Slice D — derivations are explainable
    mod explanations {
        use super::*;

        fn chain_of_three() -> Vec<Flake> {
            vec![
                f(a("thing"), rdf_type(), a("C1")),
                f(a("C1"), sub_class_of(), a("C2")),
                f(a("C2"), sub_class_of(), a("C3")),
            ]
        }

        /// The recursive property. A one-level explanation names
        /// `thing type C2` as a premise and stops — which tells the reader
        /// nothing about *why* that held, and it is the interesting half.
        #[test]
        fn a_depth_three_derivation_explains_all_the_way_down_to_assertions() {
            let facts = chain_of_three();
            let reasoning = derive(&facts);
            let target = f(a("thing"), rdf_type(), a("C3"));

            let Explanation::Derived { chains } = explain(&reasoning, &facts, &target) else {
                panic!("a derived fact must explain as derived");
            };

            assert_eq!(chains.len(), 1);
            let chain = &chains[0];
            assert_eq!(chain.rule, RuleName::SubClassOf);
            // One premise is the asserted axiom; the other is itself derived,
            // and its own explanation must be attached rather than assumed.
            let derived_premise = chain
                .premises
                .iter()
                .find(|p| matches!(p, Explanation::Derived { .. }))
                .expect("thing type C2 was derived, and the chain must say so");
            let Explanation::Derived { chains: inner } = derived_premise else {
                unreachable!()
            };
            assert_eq!(inner[0].rule, RuleName::SubClassOf);
            assert!(
                inner[0]
                    .premises
                    .iter()
                    .all(|p| matches!(p, Explanation::Asserted(_))),
                "depth 2 rests on assertions: {inner:#?}"
            );
        }

        #[test]
        fn an_asserted_fact_explains_as_asserted_rather_than_as_a_chain() {
            let facts = chain_of_three();
            let reasoning = derive(&facts);

            let explanation = explain(&reasoning, &facts, &facts[0]);

            assert!(
                matches!(explanation, Explanation::Asserted(_)),
                "{explanation:#?}"
            );
        }

        /// The negative that stops "explain everything as asserted" passing:
        /// a fact nobody stated and nothing implies is unknown, and the API
        /// turns that into a 404 rather than an empty chain.
        #[test]
        fn a_fact_that_is_neither_asserted_nor_derived_is_unknown() {
            let facts = chain_of_three();
            let reasoning = derive(&facts);

            let explanation = explain(&reasoning, &facts, &f(a("thing"), rdf_type(), a("C9")));

            assert!(
                matches!(explanation, Explanation::Unknown),
                "{explanation:#?}"
            );
        }

        #[test]
        fn a_fact_derivable_two_ways_explains_both_ways() {
            let facts = vec![
                f(a("knows"), rdf_type(), symmetric()),
                f(a("friendOf"), sub_property_of(), a("knows")),
                f(a("bob"), a("knows"), a("alice")),
                f(a("alice"), a("friendOf"), a("bob")),
            ];
            let reasoning = derive(&facts);

            let explanation = explain(&reasoning, &facts, &f(a("alice"), a("knows"), a("bob")));

            let Explanation::Derived { chains } = explanation else {
                panic!("should be derived")
            };
            assert_eq!(chains.len(), 2, "{chains:#?}");
        }

        /// **Minimum, not product.** Reasoning does not compound uncertainty
        /// the way independent sources do: a conclusion drawn from two facts
        /// the extractor was 90% sure of is 90% certain, not 81%. Under the
        /// product rule a depth-4 derivation from good premises scores 0.66 and
        /// looks worthless, which is how explainable inference gets switched
        /// off.
        #[test]
        fn confidence_is_the_minimum_of_the_premises_not_their_product() {
            let unconfirmed = |s: Sid, p: Sid, o: Sid| Flake {
                cx: Some(Sid::dsc("graph:extraction")),
                ..f(s, p, o)
            };
            let facts = vec![
                unconfirmed(a("thing"), rdf_type(), a("C1")),
                unconfirmed(a("C1"), sub_class_of(), a("C2")),
                unconfirmed(a("C2"), sub_class_of(), a("C3")),
            ];
            let budget = Budget {
                named_graph_confidence: 0.9,
                include_graphs: vec![Sid::dsc("graph:extraction")],
                ..Budget::default()
            };

            let reasoning = derive_within(&facts, &budget);
            let deep = reasoning
                .facts
                .iter()
                .find(|d| d.fact.o == FlakeValue::Ref(a("C3")))
                .expect("depth 3 should derive");

            assert!(
                (deep.confidence - 0.9).abs() < f64::EPSILON,
                "min of 0.9 premises is 0.9, not {} — that is the product rule",
                deep.confidence
            );
        }

        /// And the negative: a conclusion is never more certain than its
        /// weakest premise, so mixing a certain fact with an uncertain one
        /// yields the uncertain one's confidence rather than 1.0.
        #[test]
        fn one_uncertain_premise_is_enough_to_lower_a_conclusion() {
            let facts = vec![
                Flake {
                    cx: Some(Sid::dsc("graph:extraction")),
                    ..f(a("thing"), rdf_type(), a("C1"))
                },
                f(a("C1"), sub_class_of(), a("C2")),
            ];
            let budget = Budget {
                named_graph_confidence: 0.6,
                include_graphs: vec![Sid::dsc("graph:extraction")],
                ..Budget::default()
            };

            let reasoning = derive_within(&facts, &budget);

            assert!((reasoning.facts[0].confidence - 0.6).abs() < f64::EPSILON);
        }
    }

    /// # Slice E — the overlay is separate, replaceable, and opt-in
    mod the_overlay {
        use super::*;

        /// Derived facts are stamped into `graph:reasoning`. Writing them to
        /// the default graph is the failure mode that silently corrupts
        /// asserted data — and it cannot be undone by a later run, because a
        /// run that replaces the reasoning graph would then delete assertions.
        #[test]
        fn every_derived_fact_lands_in_the_reasoning_graph() {
            let facts = vec![
                f(a("thing"), rdf_type(), a("C1")),
                f(a("C1"), sub_class_of(), a("C2")),
            ];

            let reasoning = derive(&facts);

            assert!(!reasoning.facts.is_empty());
            assert!(
                reasoning
                    .facts
                    .iter()
                    .all(|d| d.fact.cx == Some(Sid::dsc("graph:reasoning"))),
                "{:#?}",
                reasoning.facts
            );
        }

        /// Unconfirmed extractions do not feed inference unless a deployment
        /// says they may. Reasoning over them launders a guess into a
        /// conclusion that looks like catalog truth.
        #[test]
        fn extraction_facts_derive_nothing_by_default() {
            let facts = vec![
                Flake {
                    cx: Some(Sid::dsc("graph:extraction")),
                    ..f(a("thing"), rdf_type(), a("C1"))
                },
                f(a("C1"), sub_class_of(), a("C2")),
            ];

            assert!(derive(&facts).facts.is_empty());
        }

        /// And the opt-in works, or the flag above would be indistinguishable
        /// from the rule being broken.
        #[test]
        fn an_included_graph_does_feed_inference() {
            let facts = vec![
                Flake {
                    cx: Some(Sid::dsc("graph:extraction")),
                    ..f(a("thing"), rdf_type(), a("C1"))
                },
                f(a("C1"), sub_class_of(), a("C2")),
            ];
            let budget = Budget {
                include_graphs: vec![Sid::dsc("graph:extraction")],
                ..Budget::default()
            };

            assert_eq!(derive_within(&facts, &budget).facts.len(), 1);
        }

        /// Disabling a rule removes its derivations — the property that makes
        /// the rule set a configuration rather than a constant.
        #[test]
        fn a_disabled_rule_derives_nothing() {
            let facts = vec![
                f(a("thing"), rdf_type(), a("C1")),
                f(a("C1"), sub_class_of(), a("C2")),
            ];
            let budget = Budget {
                rules: RuleName::ALL
                    .iter()
                    .copied()
                    .filter(|r| *r != RuleName::SubClassOf)
                    .collect(),
                ..Budget::default()
            };

            assert!(derive_within(&facts, &budget).facts.is_empty());
            // And the negative: the other rules still run, so the assertion
            // above is about `SubClassOf` rather than about an empty rule set.
            assert!(!derive(&facts).facts.is_empty());
        }
    }

    /// # The work a run does, and what it accounts itself at
    ///
    /// Neither of these changes an answer, which is exactly why they need
    /// tests: a reasoner that quietly evaluates naively returns the right
    /// facts and takes minutes, and a memory limit whose accounting is wrong
    /// admits the run it exists to refuse.
    mod the_cost_of_a_run {
        use super::*;

        /// Every rule reports the pairs it examined. A rule whose counter never
        /// moves makes the semi-naive assertions above pass for the wrong
        /// reason — they compare against a threshold, and zero is under every
        /// threshold.
        #[test]
        fn each_of_the_eight_rules_counts_the_pairs_it_examines() {
            let only = |rule: RuleName| Budget {
                rules: vec![rule],
                ..Budget::default()
            };
            let cases: [(RuleName, Vec<Flake>); 8] = [
                (
                    RuleName::SubClassOf,
                    vec![
                        f(a("thing"), rdf_type(), a("C1")),
                        f(a("C1"), sub_class_of(), a("C2")),
                    ],
                ),
                (
                    RuleName::SubPropertyOf,
                    vec![
                        f(a("friendOf"), sub_property_of(), a("knows")),
                        f(a("x"), a("friendOf"), a("y")),
                    ],
                ),
                (
                    RuleName::Transitive,
                    vec![
                        f(a("partOf"), rdf_type(), transitive()),
                        f(a("finger"), a("partOf"), a("hand")),
                        f(a("hand"), a("partOf"), a("arm")),
                    ],
                ),
                (
                    RuleName::Symmetric,
                    vec![
                        f(a("marriedTo"), rdf_type(), symmetric()),
                        f(a("x"), a("marriedTo"), a("y")),
                    ],
                ),
                (
                    RuleName::InverseOf,
                    vec![
                        f(a("parentOf"), inverse_of(), a("childOf")),
                        f(a("x"), a("parentOf"), a("y")),
                    ],
                ),
                (
                    RuleName::Domain,
                    vec![
                        f(a("owns"), domain(), a("Owner")),
                        f(a("x"), a("owns"), a("y")),
                    ],
                ),
                (
                    RuleName::Range,
                    vec![
                        f(a("owns"), range(), a("Asset")),
                        f(a("x"), a("owns"), a("y")),
                    ],
                ),
                (
                    RuleName::SameAs,
                    vec![f(a("x"), same_as(), a("y")), f(a("x"), a("knows"), a("z"))],
                ),
            ];

            for (rule, facts) in cases {
                let reasoning = derive_within(&facts, &only(rule));
                assert!(
                    !reasoning.facts.is_empty(),
                    "{rule:?} derived nothing, so its join count proves nothing"
                );
                assert!(reasoning.joins > 0, "{rule:?} examined no pairs");
            }
        }

        /// The accounting is per fact and per premise, not a constant. A
        /// constant passes every "did it cap" test while making the limit fire
        /// at a working-set size nobody chose.
        #[test]
        fn the_working_set_is_accounted_in_proportion_to_what_is_held() {
            let mut facts = vec![f(a("thing"), rdf_type(), a("C0"))];
            for level in 0..8 {
                facts.push(f(
                    a(&format!("C{level}")),
                    sub_class_of(),
                    a(&format!("C{}", level + 1)),
                ));
            }

            let reasoning = derive(&facts);

            let per_fact = reasoning.accounted_bytes / reasoning.facts.len();
            assert!(
                per_fact >= size_of::<Flake>(),
                "a conclusion cannot be held in less than a flake: {per_fact}"
            );
            // Deliberately an over-estimate, but a bounded one: the point is to
            // refuse before exhaustion, not to reserve the heap.
            assert!(
                per_fact <= 20 * size_of::<Flake>(),
                "{per_fact} bytes per conclusion is not an estimate, it is a guess"
            );
        }

        /// Provenance is part of what a run holds. A conclusion resting on two
        /// facts costs more to keep than one resting on one, and accounting
        /// that ignores premises under-counts exactly the structure that makes
        /// derivations explainable.
        #[test]
        fn accounting_grows_with_the_premises_a_conclusion_rests_on() {
            // Symmetry needs one premise; subsumption needs two.
            let one = derive(&[
                f(a("marriedTo"), rdf_type(), symmetric()),
                f(a("x"), a("marriedTo"), a("y")),
            ]);
            let two = derive(&[
                f(a("thing"), rdf_type(), a("C1")),
                f(a("C1"), sub_class_of(), a("C2")),
            ]);

            assert_eq!(one.facts.len(), 1);
            assert_eq!(two.facts.len(), 1);
            assert!(
                two.accounted_bytes >= one.accounted_bytes + size_of::<Flake>(),
                "one extra premise must cost about a flake: {} vs {}",
                two.accounted_bytes,
                one.accounted_bytes
            );
        }

        /// The limit admits a run that fits and stops one that does not — the
        /// pair, because either half alone passes under an accounting that is
        /// uniformly too large or uniformly too small.
        #[test]
        fn the_memory_limit_admits_what_fits_and_stops_what_does_not() {
            let mut facts = vec![f(a("thing"), rdf_type(), a("C0"))];
            for level in 0..10 {
                facts.push(f(
                    a(&format!("C{level}")),
                    sub_class_of(),
                    a(&format!("C{}", level + 1)),
                ));
            }

            let whole = derive(&facts);
            assert_eq!(whole.capped, None, "the default budget must admit this");

            let tight = derive_within(
                &facts,
                &Budget {
                    max_memory_bytes: whole.accounted_bytes / 2,
                    ..Budget::default()
                },
            );

            assert_eq!(tight.capped, Some(CappedReason::Memory));
            assert!(
                tight.facts.len() < whole.facts.len(),
                "a cap that stops nothing is not a cap"
            );
        }

        /// **The completeness hole the flag version left.** A rule with two
        /// premises must also fire when the *axiom* is the new fact and the
        /// data is old — here `sameAs` is itself derived in iteration 1, and
        /// the fact it licenses copying was asserted before the run began.
        #[test]
        fn a_two_premise_rule_fires_on_a_new_axiom_over_old_data() {
            let facts = vec![
                f(a("alias"), sub_property_of(), same_as()),
                f(a("p1"), a("alias"), a("p2")),
                f(a("p1"), a("knows"), a("q")),
            ];

            let reasoning = derive(&facts);

            assert!(
                reasoning.facts.iter().any(|d| d.fact.s == a("p2")
                    && d.fact.p == a("knows")
                    && d.fact.o == FlakeValue::Ref(a("q"))),
                "the identity arrived mid-run and must still reach old data: {:#?}",
                reasoning.facts
            );
        }
    }
}
