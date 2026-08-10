//! Namespace resolution — shipped vocabularies plus whatever a deployment
//! registers at runtime.
//!
//! **Why this module exists.** [`Sid::from_iri`](crate::flake::Sid::from_iri)
//! resolves an IRI against a fixed, compile-time array of the vocabularies
//! this binary ships with, and [`namespace_iri`](crate::flake::namespace_iri)
//! is a `match` returning `&'static str`. Both are correct for the shipped
//! set and impossible to extend without editing this crate — so a domain that
//! needs its own vocabulary has, historically, got one by adding a constant
//! here. `namespace::CUI`, `SNOMED_CT` and `RXNORM` are exactly that: three
//! namespaces added to core for one domain's ingestion work.
//!
//! That is the pattern this module ends. A namespace registered at runtime
//! gets a code from the `1024..` range [`namespace::RUNTIME_START`] already
//! reserves, and resolves through the same longest-prefix rule as a shipped
//! one — so a pack's `hosp:`, `auto:` or `gst:` IRIs become real graph
//! subjects and predicates with no code change anywhere.
//!
//! **This crate stays pure.** The registry is a trait plus an in-memory
//! implementation; loading it from a table is the storage adapter's job
//! (`00e` dependency rule 4 — `graph-owl-core` takes no I/O dependency).
//!
//! **The shipped path is unchanged and still allocation-free.**
//! `Sid::from_iri` keeps its fixed-array scan; only a caller that opts into
//! runtime namespaces pays for the trait's iteration. A binary that never
//! registers one behaves exactly as before, which is what makes this additive
//! rather than a rewrite of the hot path.

use std::collections::BTreeMap;

use crate::flake::{Sid, namespace, namespace_iri};

/// Every vocabulary the binary ships with, in the order
/// [`Sid::from_iri`](crate::flake::Sid::from_iri) scans them.
///
/// Kept beside the resolver rather than inside it so the equivalence
/// "`StaticNamespaces` resolves exactly what `Sid::from_iri` resolves" is a
/// property a test can assert, not a convention two lists have to honour.
pub const SHIPPED: [u16; 15] = [
    namespace::DSC,
    namespace::RDF,
    namespace::RDFS,
    namespace::XSD,
    namespace::OWL,
    namespace::SHACL,
    namespace::SCHEMA,
    namespace::DCTERMS,
    namespace::DCAT,
    namespace::PROV,
    namespace::FOAF,
    namespace::SKOS,
    namespace::CUI,
    namespace::SNOMED_CT,
    namespace::RXNORM,
];

/// Why a namespace could not be registered.
///
/// Every variant is a rule that protects historical flakes: a `Sid` is stored
/// as a bare `(code, local)` pair, so a code that changes meaning silently
/// rewrites every flake already carrying it — and time travel makes that
/// corruption permanent rather than transient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// Codes below [`namespace::RUNTIME_START`] belong to the binary.
    ///
    /// Allowing a deployment to claim one would let it redefine `dsc:` or
    /// `rdf:` for its own flakes, which is the same failure the `predicates`
    /// table's `core` column exists to prevent one level down.
    ReservedCode(u16),
    /// This code is already registered to a different IRI.
    ///
    /// Refused rather than overwritten: the code is already stored in flakes
    /// that mean the old IRI, and reassigning it changes what they say without
    /// touching them.
    CodeInUse {
        /// The code that was already taken.
        code: u16,
        /// What it already means.
        existing: String,
    },
    /// This IRI already belongs to another code.
    ///
    /// Refused because resolution would otherwise depend on load order: the
    /// same IRI would become one code or the other according to which
    /// registration happened to run first.
    IriInUse {
        /// The IRI that was already claimed.
        iri: String,
        /// The code that already owns it.
        existing: u16,
    },
    /// A namespace IRI must be non-empty — an empty prefix matches every IRI
    /// and would win longest-prefix against nothing, making resolution
    /// meaningless rather than merely wrong.
    EmptyIri,
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReservedCode(code) => write!(
                f,
                "namespace code {code} is reserved for vocabularies this binary ships; \
                 runtime codes start at {}",
                namespace::RUNTIME_START
            ),
            Self::CodeInUse { code, existing } => write!(
                f,
                "namespace code {code} already means `{existing}`; a code is never \
                 reassigned, because every flake already stored with it would change \
                 meaning silently"
            ),
            Self::IriInUse { iri, existing } => write!(
                f,
                "`{iri}` is already registered as namespace code {existing}; \
                 registering it twice would make resolution depend on load order"
            ),
            Self::EmptyIri => write!(f, "a namespace IRI must not be empty"),
        }
    }
}

impl std::error::Error for RegisterError {}

/// Resolves namespace codes to IRIs and back.
///
/// Object-safe on purpose: the storage adapter hands one of these down as
/// `&dyn NamespaceResolver`, and the resolution functions take it by reference
/// rather than being generic, so a runtime-aware call site does not
/// monomorphize the whole RDF layer.
pub trait NamespaceResolver {
    /// The IRI this code stands for, shipped or registered.
    fn iri(&self, code: u16) -> Option<&str>;

    /// Every `(code, iri)` pair this resolver knows.
    ///
    /// Used for the reverse direction, where longest-prefix matching has to
    /// see every candidate. Boxed because the trait is object-safe and the
    /// implementations iterate different shapes; the allocation lands once per
    /// reverse lookup and never on the shipped `Sid::from_iri` path.
    fn pairs(&self) -> Box<dyn Iterator<Item = (u16, &str)> + '_>;
}

/// The shipped vocabularies and nothing else.
///
/// Resolves precisely what [`Sid::from_iri`](crate::flake::Sid::from_iri)
/// resolves — asserted by a test rather than assumed, since the two would
/// otherwise be free to drift.
#[derive(Debug, Clone, Copy, Default)]
pub struct StaticNamespaces;

impl NamespaceResolver for StaticNamespaces {
    fn iri(&self, code: u16) -> Option<&str> {
        namespace_iri(code)
    }

    fn pairs(&self) -> Box<dyn Iterator<Item = (u16, &str)> + '_> {
        Box::new(
            SHIPPED
                .into_iter()
                .filter_map(|code| namespace_iri(code).map(|iri| (code, iri))),
        )
    }
}

/// The shipped vocabularies plus whatever this deployment registered.
///
/// A shipped namespace always wins: `register` refuses an IRI the binary
/// already owns, so there is no precedence question to get wrong at lookup
/// time.
#[derive(Debug, Clone, Default)]
pub struct RuntimeNamespaces {
    registered: BTreeMap<u16, String>,
}

impl RuntimeNamespaces {
    /// An empty registry — resolves the shipped set only.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim `code` for `iri`.
    ///
    /// Re-registering an identical `(code, iri)` pair succeeds and changes
    /// nothing, because reloading the registry from storage must be safe to
    /// repeat — an idempotent load is the normal case, not an error.
    ///
    /// # Errors
    ///
    /// [`RegisterError`] when the code is reserved for the binary, when the
    /// code already means something else, or when the IRI already belongs to
    /// another code (shipped or registered).
    pub fn register(&mut self, code: u16, iri: impl Into<String>) -> Result<(), RegisterError> {
        let iri = iri.into();
        if iri.is_empty() {
            return Err(RegisterError::EmptyIri);
        }
        if code < namespace::RUNTIME_START {
            return Err(RegisterError::ReservedCode(code));
        }
        if let Some(existing) = self.registered.get(&code) {
            if existing == &iri {
                return Ok(());
            }
            return Err(RegisterError::CodeInUse {
                code,
                existing: existing.clone(),
            });
        }
        // Shadowing a shipped IRI is refused before a registered one, because
        // it is the more dangerous of the two: it would let a deployment
        // reinterpret `rdf:type` for its own flakes while every other part of
        // the system still reads the shipped meaning.
        if let Some(shipped) = SHIPPED
            .into_iter()
            .find(|&c| namespace_iri(c) == Some(iri.as_str()))
        {
            return Err(RegisterError::IriInUse {
                iri,
                existing: shipped,
            });
        }
        if let Some((&existing, _)) = self.registered.iter().find(|(_, v)| *v == &iri) {
            return Err(RegisterError::IriInUse { iri, existing });
        }
        self.registered.insert(code, iri);
        Ok(())
    }

    /// How many runtime namespaces are registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.registered.len()
    }

    /// Whether no runtime namespace is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.registered.is_empty()
    }
}

impl NamespaceResolver for RuntimeNamespaces {
    fn iri(&self, code: u16) -> Option<&str> {
        namespace_iri(code).or_else(|| self.registered.get(&code).map(String::as_str))
    }

    fn pairs(&self) -> Box<dyn Iterator<Item = (u16, &str)> + '_> {
        Box::new(
            StaticNamespaces
                .pairs()
                .chain(self.registered.iter().map(|(&c, i)| (c, i.as_str()))),
        )
    }
}

/// Split a full IRI into a namespace code and local name, over any resolver.
///
/// Longest-prefix wins, the same rule
/// [`Sid::from_iri`](crate::flake::Sid::from_iri) applies: two vocabulary IRIs
/// can share a prefix, and matching the shorter would silently attribute a
/// term to the wrong vocabulary.
#[must_use]
pub fn resolve_iri(iri: &str, resolver: &dyn NamespaceResolver) -> Option<Sid> {
    resolver
        .pairs()
        .filter_map(|(code, base)| {
            iri.strip_prefix(base)
                .map(|local| (base.len(), code, local))
        })
        .max_by_key(|(len, _, _)| *len)
        .map(|(_, code, local)| Sid::new(code, local))
}

/// The full IRI a `Sid` denotes, over any resolver.
///
/// `None` when the namespace has no assigned IRI — a `Sid` that cannot be
/// expressed as an IRI must fail loudly rather than serialize as a bare local
/// name, which would silently drop the vocabulary it belongs to.
#[must_use]
pub fn sid_to_iri(sid: &Sid, resolver: &dyn NamespaceResolver) -> Option<String> {
    resolver
        .iri(sid.namespace_code)
        .map(|base| format!("{base}{}", sid.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOSP: u16 = namespace::RUNTIME_START;
    const HOSP_IRI: &str = "https://example.org/ns/hospitality#";

    fn hospitality() -> RuntimeNamespaces {
        let mut ns = RuntimeNamespaces::new();
        ns.register(HOSP, HOSP_IRI).expect("a fresh code is free");
        ns
    }

    // ── the gap this module closes ──────────────────────────────────────

    #[test]
    fn a_domain_iri_is_unresolvable_without_a_registry() {
        // Characterises the behaviour that motivated this module: today a
        // pack's own vocabulary cannot become a graph subject at all, which
        // is why the last domain that needed one got three constants added
        // to `graph-owl-core` instead.
        assert_eq!(Sid::from_iri(&format!("{HOSP_IRI}Property")), None);
    }

    #[test]
    fn a_registered_domain_iri_resolves_to_its_own_namespace() {
        let sid = resolve_iri(&format!("{HOSP_IRI}Property"), &hospitality())
            .expect("a registered namespace resolves");

        assert_eq!(sid, Sid::new(HOSP, "Property"));
    }

    #[test]
    fn registering_a_domain_namespace_leaves_the_shipped_ones_untouched() {
        // The negative half: a registry that resolved its own namespace but
        // broke `rdf:` would pass the test above and be unusable.
        let sid = resolve_iri(
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
            &hospitality(),
        )
        .expect("shipped namespaces still resolve");

        assert_eq!(sid, Sid::new(namespace::RDF, "type"));
    }

    // ── the static resolver is exactly today's behaviour ────────────────

    #[test]
    fn the_static_resolver_matches_sid_from_iri_for_every_shipped_namespace() {
        // The equivalence that lets `Sid::from_iri` keep its fast fixed-array
        // scan: if these two ever disagree, one call site resolves an IRI
        // differently from another, which is the worst possible failure here
        // because both look correct in isolation.
        for code in SHIPPED {
            let base = namespace_iri(code).expect("every shipped code has an IRI");
            let iri = format!("{base}Thing");

            assert_eq!(
                resolve_iri(&iri, &StaticNamespaces),
                Sid::from_iri(&iri),
                "disagreement on {base}"
            );
        }
    }

    #[test]
    fn an_unregistered_iri_resolves_to_nothing_in_either_resolver() {
        let stray = "https://nobody.example/ns#Thing";

        assert_eq!(resolve_iri(stray, &StaticNamespaces), None);
        assert_eq!(resolve_iri(stray, &hospitality()), None);
    }

    // ── round trip, both directions, both kinds of namespace ────────────

    #[test]
    fn a_runtime_sid_round_trips_through_its_iri() {
        let ns = hospitality();
        let sid = Sid::new(HOSP, "Property");

        let iri = sid_to_iri(&sid, &ns).expect("a registered namespace has an IRI");

        assert_eq!(iri, format!("{HOSP_IRI}Property"));
        assert_eq!(resolve_iri(&iri, &ns), Some(sid));
    }

    #[test]
    fn a_shipped_sid_round_trips_through_the_runtime_resolver_too() {
        let ns = hospitality();
        let sid = Sid::new(namespace::DCAT, "Dataset");

        let iri = sid_to_iri(&sid, &ns).expect("a shipped namespace has an IRI");

        assert_eq!(resolve_iri(&iri, &ns), Some(sid));
    }

    #[test]
    fn an_unregistered_code_has_no_iri() {
        assert_eq!(sid_to_iri(&Sid::new(9999, "x"), &hospitality()), None);
    }

    // ── longest-prefix, which is why resolution is not a simple map ─────

    #[test]
    fn the_longer_of_two_overlapping_prefixes_wins() {
        // Two registered vocabularies where one's IRI is a prefix of the
        // other's. Matching the shorter would file `.../rooms#Suite` under
        // the parent vocabulary — a term silently attributed to the wrong
        // ontology, which no later validation would catch.
        let mut ns = RuntimeNamespaces::new();
        ns.register(1024, "https://example.org/ns/").expect("free");
        ns.register(1025, "https://example.org/ns/rooms#")
            .expect("free");

        assert_eq!(
            resolve_iri("https://example.org/ns/rooms#Suite", &ns),
            Some(Sid::new(1025, "Suite")),
            "the longer prefix must win"
        );
        assert_eq!(
            resolve_iri("https://example.org/ns/Other", &ns),
            Some(Sid::new(1024, "Other")),
            "and the shorter must still serve what only it matches"
        );
    }

    // ── the rules that protect flakes already stored ────────────────────

    #[test]
    fn a_reserved_code_cannot_be_claimed_at_runtime() {
        let mut ns = RuntimeNamespaces::new();

        assert_eq!(
            ns.register(namespace::DSC, "https://evil.example/ns#"),
            Err(RegisterError::ReservedCode(namespace::DSC)),
            "claiming `dsc:` would let a deployment redefine the catalog vocabulary"
        );
    }

    #[test]
    fn the_last_reserved_code_is_refused_and_the_first_runtime_code_is_not() {
        // The boundary itself, both sides — an off-by-one here either locks
        // out the whole runtime range or hands out the last shipped code.
        let mut ns = RuntimeNamespaces::new();

        assert!(
            ns.register(namespace::RUNTIME_START - 1, "https://a.example/#")
                .is_err()
        );
        assert!(
            ns.register(namespace::RUNTIME_START, "https://b.example/#")
                .is_ok()
        );
    }

    #[test]
    fn a_code_is_never_reassigned_to_a_different_iri() {
        let mut ns = hospitality();

        assert_eq!(
            ns.register(HOSP, "https://example.org/ns/something-else#"),
            Err(RegisterError::CodeInUse {
                code: HOSP,
                existing: HOSP_IRI.to_string(),
            }),
            "every flake already stored with this code would change meaning"
        );
    }

    #[test]
    fn re_registering_the_same_pair_is_idempotent_not_an_error() {
        // Reloading the registry from storage is the normal case; a load that
        // failed the second time would make a restart a failure.
        let mut ns = hospitality();

        assert_eq!(ns.register(HOSP, HOSP_IRI), Ok(()));
        assert_eq!(ns.len(), 1);
    }

    #[test]
    fn a_shipped_iri_cannot_be_shadowed_by_a_runtime_code() {
        let mut ns = RuntimeNamespaces::new();

        assert_eq!(
            ns.register(2048, "http://www.w3.org/2002/07/owl#"),
            Err(RegisterError::IriInUse {
                iri: "http://www.w3.org/2002/07/owl#".to_string(),
                existing: namespace::OWL,
            }),
            "two codes for `owl:` would make resolution depend on load order"
        );
    }

    #[test]
    fn one_iri_cannot_be_registered_under_two_runtime_codes() {
        let mut ns = hospitality();

        assert_eq!(
            ns.register(2048, HOSP_IRI),
            Err(RegisterError::IriInUse {
                iri: HOSP_IRI.to_string(),
                existing: HOSP,
            })
        );
    }

    #[test]
    fn an_empty_iri_is_refused() {
        // An empty prefix strips from every IRI, so it would match everything
        // and win nothing — resolution would stop meaning anything at all.
        let mut ns = RuntimeNamespaces::new();

        assert_eq!(ns.register(1024, ""), Err(RegisterError::EmptyIri));
    }

    #[test]
    fn a_refused_registration_leaves_the_registry_unchanged() {
        let mut ns = hospitality();

        let _ = ns.register(HOSP, "https://example.org/ns/other#");
        let _ = ns.register(namespace::DSC, "https://example.org/ns/x#");
        let _ = ns.register(2048, HOSP_IRI);

        assert_eq!(ns.len(), 1);
        assert_eq!(ns.iri(HOSP), Some(HOSP_IRI));
    }

    // ── the counting and emptiness negatives ────────────────────────────
    //
    // Every assertion above happens to be against a registry holding exactly
    // one namespace, so `len()` returning a constant `1` and `is_empty()`
    // returning a constant `true` both survived the first mutation run. The
    // fix is the same one this project keeps rediscovering: for every "X
    // reports N", also assert "and Y does not".

    #[test]
    fn the_registry_counts_what_it_holds_at_zero_one_and_many() {
        let mut ns = RuntimeNamespaces::new();
        assert_eq!(ns.len(), 0);

        ns.register(1024, "https://a.example/#").expect("free");
        assert_eq!(ns.len(), 1);

        ns.register(1025, "https://b.example/#").expect("free");
        assert_eq!(ns.len(), 2);
    }

    #[test]
    fn a_registry_holding_a_namespace_is_not_empty() {
        assert!(!hospitality().is_empty());
    }

    // ── the static resolver's forward direction ─────────────────────────

    #[test]
    fn the_static_resolver_maps_a_shipped_code_to_its_own_iri() {
        // `StaticNamespaces::iri` was reachable only through `pairs()` until
        // now, so returning `None` — or any constant string — for every code
        // survived. `sid_to_iri(.., &StaticNamespaces)` is a real call path
        // for a binary that registers nothing, and it has to be right.
        assert_eq!(
            StaticNamespaces.iri(namespace::OWL),
            Some("http://www.w3.org/2002/07/owl#"),
        );
        assert_eq!(
            StaticNamespaces.iri(namespace::DCAT),
            Some("http://www.w3.org/ns/dcat#"),
            "and two different codes must not share one answer",
        );
        assert_eq!(
            StaticNamespaces.iri(namespace::RUNTIME_START),
            None,
            "a code the binary never shipped has no IRI here",
        );
    }

    #[test]
    fn a_shipped_sid_renders_its_iri_through_the_static_resolver() {
        assert_eq!(
            sid_to_iri(&Sid::new(namespace::PROV, "Activity"), &StaticNamespaces),
            Some("http://www.w3.org/ns/prov#Activity".to_string()),
        );
    }

    // ── the refusal messages an operator actually reads ─────────────────

    #[test]
    fn each_refusal_explains_itself_in_terms_of_what_it_protects() {
        // These messages are the only place the *reason* reaches an operator
        // staring at a failed pack load. A `Display` impl that rendered
        // nothing would leave "registration failed" and no way to act on it.
        assert!(
            RegisterError::ReservedCode(namespace::DSC)
                .to_string()
                .contains("reserved"),
        );
        assert!(
            RegisterError::CodeInUse {
                code: HOSP,
                existing: HOSP_IRI.to_string(),
            }
            .to_string()
            .contains(HOSP_IRI),
            "the message must name what the code already means",
        );
        assert!(
            RegisterError::IriInUse {
                iri: HOSP_IRI.to_string(),
                existing: HOSP,
            }
            .to_string()
            .contains(HOSP_IRI),
        );
        assert!(!RegisterError::EmptyIri.to_string().is_empty());
    }

    #[test]
    fn an_empty_registry_resolves_exactly_the_shipped_set() {
        let ns = RuntimeNamespaces::new();

        assert!(ns.is_empty());
        assert_eq!(
            resolve_iri("http://www.w3.org/ns/dcat#Dataset", &ns),
            Sid::from_iri("http://www.w3.org/ns/dcat#Dataset"),
        );
    }
}
