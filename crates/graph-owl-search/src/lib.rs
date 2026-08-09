//! What is searched, and how a person's typing becomes a query.
//!
//! This crate owns the *question*; an adapter owns how it is answered. The one
//! thing that must not live in an adapter is the translation from what a user
//! typed into a query, because two adapters translating it differently would
//! give the same search box two meanings.
//!
//! **The `TextIndex` port is deliberately absent** (decision, 28 July 2026). A
//! port abstracts a *detached* index — one living outside the write transaction,
//! able to drift from its source, needing events to catch up. The Postgres
//! backend has none: its search vector is a generated column on the row it
//! describes, written in the same transaction, so there is nothing to reindex,
//! retry or reconcile. Introducing a port now would mean inventing a seam with
//! exactly one implementation on either side of it. It becomes necessary when a
//! detached engine lands (`08-engine-search.md` Slice A, `OpenSearch`; Epic 33's
//! vector index), and the shape it should take is knowable then rather than now.
//!
//! `VectorIndex` (an ANN index over many vectors) is still not here — nothing
//! in this codebase needs approximate nearest-neighbour search over an
//! unbounded corpus yet. What now exists is [`embeddings`]: the process
//! boundary itself, for Epic 31's semantic ranking term, which only ever
//! reranks a small, already-filtered candidate set and has no need of an
//! index.

pub mod embeddings;

/// Turn what a person typed into a Postgres `tsquery` expression.
///
/// Every term is prefix-matched and the terms are `AND`ed. Prefix matching is what
/// makes a search box feel like a search box: `upi trans` has to find
/// `upi_transactions` while the user is still typing, and a full-word match
/// would find nothing until the last character. `AND` rather than `OR` is the
/// narrowing behaviour people expect — each word typed should shrink the result
/// set, not grow it.
///
/// **Everything outside `[A-Za-z0-9]` is a separator**, which does three jobs at
/// once. It splits identifiers, so `upi_transactions` typed as a query becomes
/// the same two terms it becomes in the index. It makes the output safe to hand
/// to `to_tsquery`, which raises a syntax error on stray `&`, `|`, `!`, `(`,
/// `)` or `:` — an error surfacing as a failed search rather than as no
/// results. And it means the function cannot be made to emit operators, so the
/// caller's parameter binding is not the only thing standing between a user's
/// input and the query language.
///
/// `None` when nothing survives — an all-punctuation query has no terms, and
/// `to_tsquery('english', '')` is an error rather than a query matching
/// nothing. The caller answers an empty result instead of asking.
#[must_use]
pub fn tsquery(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(|term| format!("{}:*", term.to_lowercase()))
        .collect();

    if terms.is_empty() {
        return None;
    }
    Some(terms.join(" & "))
}

#[cfg(test)]
mod tests {
    use super::*;

    mod what_a_person_typed_becomes_a_query {
        use super::*;

        /// Terms are passed through unstemmed. `to_tsquery('english', …)`
        /// stems them, so `customers:*` becomes the lexeme `customer:*` inside
        /// Postgres — stemming here as well would apply an English stemmer
        /// twice, and a second-hand approximation of Postgres's own rules is
        /// the kind of drift that shows up as one term in a hundred not
        /// matching.
        #[test]
        fn one_word_becomes_one_prefix_term_left_unstemmed() {
            assert_eq!(tsquery("customers"), Some("customers:*".to_string()));
        }

        #[test]
        fn several_words_are_anded_so_each_one_narrows() {
            // Not ORed. Typing a second word must shrink the results, or the
            // search box rewards vagueness.
            assert_eq!(tsquery("upi trans"), Some("upi:* & trans:*".to_string()));
        }

        #[test]
        fn an_identifier_splits_the_same_way_the_index_splits_it() {
            // `upi_transactions` is indexed as two lexemes, so a query typed
            // with the underscore has to become the same two.
            assert_eq!(
                tsquery("upi_transactions"),
                Some("upi:* & transactions:*".to_string())
            );
        }

        #[test]
        fn a_dotted_fqn_splits_into_its_parts() {
            assert_eq!(
                tsquery("hdfc-core.retail.core_banking"),
                Some("hdfc:* & core:* & retail:* & core:* & banking:*".to_string())
            );
        }

        #[test]
        fn case_is_folded_because_nobody_types_the_case_of_a_column() {
            assert_eq!(tsquery("UPI"), Some("upi:*".to_string()));
        }
    }

    mod nothing_a_user_can_type_reaches_the_query_language {
        use super::*;

        /// The negative that matters: an operator typed into the search box
        /// must arrive as a *separator*, never as an operator. Otherwise a
        /// query like `a & b` is a syntax error the user sees as a broken
        /// search, and `!x` silently inverts their intent.
        #[test]
        fn operators_are_separators_not_operators() {
            assert_eq!(tsquery("a & b"), Some("a:* & b:*".to_string()));
            assert_eq!(tsquery("!upi"), Some("upi:*".to_string()));
            assert_eq!(tsquery("(a|b)"), Some("a:* & b:*".to_string()));
            assert_eq!(tsquery("x:*"), Some("x:*".to_string()));
        }

        #[test]
        fn no_output_ever_contains_a_bare_operator_character() {
            for typed in ["a&b", "a|b", "!a", "(a)", "a:b", "'a'", "a\\b"] {
                let produced = tsquery(typed).expect("has letters");
                let operators: String = produced
                    .replace(":*", "")
                    .replace(" & ", "")
                    .chars()
                    .filter(|c| !c.is_ascii_alphanumeric())
                    .collect();
                assert!(
                    operators.is_empty(),
                    "{typed:?} produced {produced:?}, leaking {operators:?}"
                );
            }
        }
    }

    mod a_query_with_no_terms_is_not_a_query {
        use super::*;

        #[test]
        fn punctuation_alone_produces_nothing() {
            // `to_tsquery('english', '')` is an error, not a query matching
            // nothing — so the caller has to be told there is no query at all.
            assert_eq!(tsquery("!!!"), None);
        }

        #[test]
        fn the_empty_string_produces_nothing() {
            assert_eq!(tsquery(""), None);
        }

        #[test]
        fn whitespace_alone_produces_nothing() {
            assert_eq!(tsquery("   "), None);
        }

        /// And the negative: a query that *does* have a term must not be
        /// dropped. A function returning `None` too eagerly turns every search
        /// into an empty result, which looks exactly like an empty catalog.
        #[test]
        fn a_term_buried_in_punctuation_still_produces_a_query() {
            assert_eq!(tsquery("!!!upi!!!"), Some("upi:*".to_string()));
        }
    }
}
