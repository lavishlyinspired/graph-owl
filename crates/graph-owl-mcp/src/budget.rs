//! Token budgets and truncation — Epic 14 Slice E.
//!
//! **Detail goes before entities, and that ordering is the whole slice.** A
//! response that drops an entity to fit teaches an agent something false — that
//! the entity does not exist — and the agent then asserts its absence with the
//! same confidence it asserts everything else. A response that shortens a
//! description teaches it nothing false; it just knows less about something it
//! can see.
//!
//! So the order is fixed and tested: **descriptions shorten, then related-entity
//! lists shorten, then the entity list truncates last, and only ever with
//! `truncated` set.** Silent loss is the one outcome that is never acceptable.

use serde::Serialize;

/// How much room a response has.
///
/// **Measured, not estimated by character count.** A budget that counted
/// characters would be wrong by a factor that varies with the content — FQNs
/// and JSON punctuation tokenize very differently from prose — and being wrong
/// in the *generous* direction means the agent's context window overflows
/// somewhere this code cannot see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenBudget {
    pub max_tokens: usize,
}

impl Default for TokenBudget {
    /// Eight thousand: roughly a quarter of a small model's window, which
    /// leaves room for the agent's own reasoning and for several tool calls in
    /// one conversation. A budget that could fill the window on its own turns
    /// every multi-step investigation into a context overflow, and the whole
    /// point of a task-shaped tool surface is that investigations take several
    /// steps.
    fn default() -> Self {
        Self { max_tokens: 8_000 }
    }
}

/// Why a response is not the whole answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TruncationReason {
    /// Descriptions were shortened. Nothing was lost that the agent cannot ask
    /// for again by name.
    DetailShortened,
    /// Related-entity lists were cut. The entities themselves are all present.
    RelationsShortened,
    /// **Entities were dropped.** The most serious kind, and the only one that
    /// can make an agent believe something is absent — which is why it is
    /// reported distinctly rather than folded into the others.
    EntitiesDropped,
    /// A walk stopped at its depth bound. Nothing was measured or cut; the
    /// answer is simply not the whole graph.
    ///
    /// Folded in here rather than given its own field because an agent asks one
    /// question of a response — "is this everything?" — and two flags answering
    /// it separately is two flags to forget to check.
    DepthReached,
}

/// An estimate of how many tokens a serialized value costs.
///
/// **Deliberately an approximation, and deliberately conservative.** The exact
/// count depends on the model's tokenizer, which this crate has no business
/// depending on — so the rule is to *over*-count rather than under-count: being
/// wrong here in the generous direction overflows a context window somewhere
/// this code cannot observe, and being wrong in the mean direction merely sends
/// a shorter answer.
///
/// Four characters per token is the widely-used English approximation; JSON
/// punctuation and identifiers tokenize worse than prose, so this divides by
/// three instead.
#[must_use]
pub fn estimate_tokens(value: &serde_json::Value) -> usize {
    let rendered = value.to_string();
    rendered.len().div_ceil(3)
}

/// A response being fitted to a budget.
///
/// Implemented by each tool's payload so the fitting order lives here once
/// rather than in every tool — five tools with five orderings is five chances
/// to drop an entity first.
pub trait Fits {
    /// Shorten prose. Returns whether anything changed.
    fn shorten_detail(&mut self) -> bool;
    /// Cut related-entity lists. Returns whether anything changed.
    fn shorten_relations(&mut self) -> bool;
    /// Drop entities. Returns whether anything changed.
    fn drop_entities(&mut self) -> bool;
    /// The payload as it would be sent.
    fn render(&self) -> serde_json::Value;
}

/// Fit a response into a budget, cheapest loss first.
///
/// Returns the most serious truncation that was needed, or `None` when the
/// response already fitted.
///
/// **The loop re-measures after every step** rather than computing what to drop
/// up front: shortening a description changes the size of everything after it,
/// and a plan made before the first cut is a plan made against a document that
/// no longer exists.
pub fn fit<T: Fits>(payload: &mut T, budget: TokenBudget) -> Option<TruncationReason> {
    if estimate_tokens(&payload.render()) <= budget.max_tokens {
        return None;
    }

    let mut worst = None;

    // 1. Prose. The agent can ask for a description by name; it cannot ask for
    //    an entity it was never told about.
    while estimate_tokens(&payload.render()) > budget.max_tokens {
        if !payload.shorten_detail() {
            break;
        }
        worst = Some(TruncationReason::DetailShortened);
    }
    if estimate_tokens(&payload.render()) <= budget.max_tokens {
        return worst;
    }

    // 2. Related-entity lists. Every entity is still present and nameable.
    while estimate_tokens(&payload.render()) > budget.max_tokens {
        if !payload.shorten_relations() {
            break;
        }
        worst = Some(TruncationReason::RelationsShortened);
    }
    if estimate_tokens(&payload.render()) <= budget.max_tokens {
        return worst;
    }

    // 3. Entities, last, and **never silently**: the caller sets `truncated`
    //    from this return value, and an agent told the list is incomplete asks
    //    again rather than concluding absence.
    while estimate_tokens(&payload.render()) > budget.max_tokens {
        if !payload.drop_entities() {
            break;
        }
        worst = Some(TruncationReason::EntitiesDropped);
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A payload with three levers, each recording whether it was pulled — so a
    /// test can assert the *order* rather than only the outcome.
    #[derive(Debug)]
    struct Payload {
        descriptions: Vec<String>,
        relations: Vec<Vec<String>>,
        entities: Vec<String>,
    }

    impl Payload {
        fn large() -> Self {
            Self {
                descriptions: (0..6).map(|_| "x".repeat(400)).collect(),
                relations: (0..6)
                    .map(|_| (0..12).map(|n| format!("related.entity.{n}")).collect())
                    .collect(),
                entities: (0..6).map(|n| format!("svc.db.public.table_{n}")).collect(),
            }
        }
    }

    impl Fits for Payload {
        fn shorten_detail(&mut self) -> bool {
            let mut changed = false;
            for description in &mut self.descriptions {
                if !description.is_empty() {
                    description.clear();
                    changed = true;
                }
            }
            changed
        }

        fn shorten_relations(&mut self) -> bool {
            let mut changed = false;
            for list in &mut self.relations {
                if !list.is_empty() {
                    list.pop();
                    changed = true;
                }
            }
            changed
        }

        fn drop_entities(&mut self) -> bool {
            self.entities.pop().is_some()
        }

        fn render(&self) -> serde_json::Value {
            serde_json::json!({
                "descriptions": self.descriptions,
                "relations": self.relations,
                "entities": self.entities,
            })
        }
    }

    #[test]
    fn a_response_within_budget_is_untouched() {
        let mut payload = Payload::large();
        let before = payload.render();

        let reason = fit(
            &mut payload,
            TokenBudget {
                max_tokens: 100_000,
            },
        );

        assert_eq!(reason, None);
        assert_eq!(payload.render(), before);
    }

    /// **The ordering test, and it is the point of the slice.** A response over
    /// budget shortens prose *before* it drops an entity — losing an entity
    /// silently is what makes an agent assert false absence.
    #[test]
    fn detail_is_shortened_before_any_entity_is_dropped() {
        let mut payload = Payload::large();
        let entities_before = payload.entities.len();
        // **The budget is measured off the fixture, not guessed.** A hand-picked
        // number encodes a size the fixture happens to have today, and the test
        // then fails for a reason that has nothing to do with the ordering it
        // exists to check. This one is exactly "what the payload costs once the
        // prose is gone" — the budget at which detail-shortening alone suffices.
        let max_tokens = {
            let mut probe = Payload::large();
            probe.shorten_detail();
            estimate_tokens(&probe.render())
        };

        let reason = fit(&mut payload, TokenBudget { max_tokens });

        assert_eq!(reason, Some(TruncationReason::DetailShortened));
        assert_eq!(
            payload.entities.len(),
            entities_before,
            "every entity survives: {payload:?}"
        );
        assert!(payload.descriptions.iter().all(String::is_empty));
    }

    /// And relations go before entities, for the same reason one step down: the
    /// entities are all still nameable.
    #[test]
    fn relations_are_shortened_before_any_entity_is_dropped() {
        let mut payload = Payload::large();
        let entities_before = payload.entities.len();
        // Measured the same way: what it costs with prose gone *and* every
        // relation list emptied — one rung further down than the test above.
        let max_tokens = {
            let mut probe = Payload::large();
            probe.shorten_detail();
            while probe.shorten_relations() {}
            estimate_tokens(&probe.render())
        };

        let reason = fit(&mut payload, TokenBudget { max_tokens });

        assert_eq!(reason, Some(TruncationReason::RelationsShortened));
        assert_eq!(payload.entities.len(), entities_before, "{payload:?}");
        assert!(
            payload.relations.iter().all(Vec::is_empty),
            "relations were exhausted first: {payload:?}"
        );
    }

    /// **Entities last, and the reason says so.** A caller sets `truncated`
    /// from this, and an agent told the list is incomplete asks again rather
    /// than concluding absence.
    #[test]
    fn entities_are_dropped_only_when_nothing_cheaper_is_left_and_it_is_reported() {
        let mut payload = Payload::large();

        let reason = fit(&mut payload, TokenBudget { max_tokens: 20 });

        assert_eq!(reason, Some(TruncationReason::EntitiesDropped));
        assert!(payload.entities.len() < 6, "{payload:?}");
        assert!(payload.descriptions.iter().all(String::is_empty));
        assert!(payload.relations.iter().all(Vec::is_empty));
    }

    /// **A truncated response is still valid and still structured.** An agent
    /// receiving malformed JSON learns nothing at all, which is worse than
    /// learning less.
    #[test]
    fn a_truncated_response_is_still_well_formed() {
        let mut payload = Payload::large();

        fit(&mut payload, TokenBudget { max_tokens: 20 });

        let rendered = payload.render();
        assert!(rendered.get("entities").is_some(), "{rendered}");
        assert!(rendered.get("relations").is_some(), "{rendered}");
    }

    /// A budget nothing can satisfy terminates rather than looping forever —
    /// the levers run out, and the answer is the smallest thing that can be
    /// built rather than a hang.
    #[test]
    fn an_impossible_budget_terminates_with_everything_dropped() {
        let mut payload = Payload::large();

        let reason = fit(&mut payload, TokenBudget { max_tokens: 0 });

        assert_eq!(reason, Some(TruncationReason::EntitiesDropped));
        assert!(payload.entities.is_empty());
    }

    /// The estimate over-counts rather than under-counts: being generous
    /// overflows a context window somewhere this code cannot see.
    #[test]
    fn the_estimate_is_conservative_rather_than_generous() {
        let value = serde_json::json!({ "fullyQualifiedName": "svc.db.public.orders" });
        let rendered = value.to_string();

        let estimated = estimate_tokens(&value);

        assert!(
            estimated >= rendered.len() / 4,
            "a four-chars-per-token reading would be {}, this is {estimated}",
            rendered.len() / 4
        );
    }

    #[test]
    fn a_truncation_reason_is_camel_case_on_the_wire() {
        let json = serde_json::to_value(TruncationReason::EntitiesDropped).expect("serialize");
        assert_eq!(json, "entitiesDropped");
    }
}
