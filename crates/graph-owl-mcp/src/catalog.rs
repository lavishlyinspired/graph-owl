//! The adapter: `ContextSource` over `Catalog` — Epic 14.
//!
//! Slices A and B were pure and served nothing. This is what makes them
//! reachable, and it carries one obligation the rest of the crate depends on:
//! **the catalog's fields must reach [`Observed`] without defaulting on the
//! way in.** A field that lost its `None` before arriving would have already
//! thrown away the distinction `trust` exists to preserve — and the loss is
//! invisible, because "no tests" and "tests passed" both render as a value.

use async_trait::async_trait;
use graph_owl_api::{Catalog, CatalogError};

use crate::{
    AssetContext, ContextSource, MemoryContext, SourceError,
    trust::{Observed, summarise},
};

/// Serves MCP tools from the catalog, filtered by the caller's policy.
pub struct CatalogContext {
    catalog: Catalog,
}

impl CatalogContext {
    #[must_use]
    pub fn new(catalog: Catalog) -> Self {
        Self { catalog }
    }
}

/// Any catalog failure is "we could not look", never "it is not there".
///
/// `CatalogError` is deliberately not `Display` — it carries field errors a
/// caller is meant to destructure — so the message is built here rather than
/// stringified by accident. The distinction matters more than the text: an
/// agent told `NotFound` for a failed read reports an absence it never checked.
fn unavailable(error: &CatalogError) -> SourceError {
    SourceError::Unavailable(format!("{error:?}"))
}

/// Read one property off an asset's free-form `properties`.
///
/// `None` when absent **or** when present-but-not-a-string. A property holding
/// the wrong type is a property nobody can act on, and coercing it would put a
/// rendered `{"a":1}` where a lifecycle name belongs.
fn text(asset: &graph_owl_core::Asset, key: &str) -> Option<String> {
    asset
        .properties
        .as_ref()?
        .get(key)?
        .as_str()
        .map(ToString::to_string)
}

fn flag(asset: &graph_owl_core::Asset, key: &str) -> Option<bool> {
    asset.properties.as_ref()?.get(key)?.as_bool()
}

fn instant(asset: &graph_owl_core::Asset, key: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let raw = text(asset, key)?;
    chrono::DateTime::parse_from_rfc3339(&raw)
        .ok()
        .map(|t| t.with_timezone(&chrono::Utc))
}

/// What the catalog holds about an asset, without deciding anything.
///
/// Every field is read as an `Option` and handed on as one. **No defaults
/// here**: `trust::summarise` is where absence becomes `Unknown`, and doing it
/// twice — or doing it here instead — is how "nobody tested this" quietly
/// becomes "this passed".
#[must_use]
pub fn observe(asset: &graph_owl_core::Asset, has_lineage: bool) -> Observed {
    Observed {
        lifecycle: text(asset, "lifecycle"),
        successor: text(asset, "successor"),
        // `Asset` carries no owner field today — Epic 11's `owner_id` lives on
        // the row and is not projected here — so this reads the property the
        // connectors write. **When the envelope gains one, read that instead**:
        // a typo in free-form JSON currently looks exactly like an owner.
        owner: text(asset, "owner"),
        description: asset.description.clone(),
        certified_by: text(asset, "certifiedBy"),
        certification_expires_at: instant(asset, "certificationExpiresAt"),
        tests_passing: flag(asset, "testsPassing"),
        tests_last_run_at: instant(asset, "testsLastRunAt"),
        has_lineage,
    }
}

#[async_trait]
impl ContextSource for CatalogContext {
    async fn asset_context(
        &self,
        principal: &str,
        fqn: &str,
    ) -> Result<Option<AssetContext>, SourceError> {
        // The principal is resolved rather than trusted: an MCP session names a
        // subject, and what that subject may *see* is the catalog's decision,
        // not the protocol's.
        let who = self
            .catalog
            .resolve_principal(principal, principal)
            .await
            .map_err(|e| unavailable(&e))?;

        let Some(asset) = self
            .catalog
            .get_asset_by_fqn(fqn)
            .await
            .map_err(|e| unavailable(&e))?
        else {
            return Ok(None);
        };

        // **Policy decides visibility, and a denial is an absence.** The
        // filtered read is reused rather than a second rule written here: two
        // implementations of "may this principal see it" is one more than can
        // be kept in step, and this is the copy an agent drives.
        //
        // `NotFound` from the facade covers both "no such asset" and "not for
        // you" — which is the property `Outcome::NotFound` rests on.
        if self.catalog.get_asset_for(&who, asset.id).await.is_err() {
            return Ok(None);
        }

        // The difference between what exists and what this principal may see
        // *is* the withheld count. Asking the question twice is the only way to
        // know something was hidden, because a filtered read cannot report what
        // it removed.
        let all = self
            .catalog
            .list_children(Some(asset.id))
            .await
            .map_err(|e| unavailable(&e))?;
        let visible = self
            .catalog
            .list_children_for(&who, Some(asset.id))
            .await
            .map_err(|e| unavailable(&e))?;

        Ok(Some(AssetContext {
            fully_qualified_name: asset.fully_qualified_name.clone(),
            kind: asset.kind.to_string(),
            description: asset.description.clone(),
            related: visible
                .iter()
                .map(|child| child.fully_qualified_name.clone())
                .collect(),
            // **Set when policy withheld something**, not when nothing was
            // found. An agent that cannot tell a complete answer from a
            // filtered one presents the filtered one as complete.
            policy_filtered: visible.len() < all.len(),
            trust: summarise(&observe(&asset, false), chrono::Utc::now()),
        }))
    }

    async fn recall(
        &self,
        principal: &str,
        fqn: &str,
        query: &str,
    ) -> Result<Option<Vec<MemoryContext>>, SourceError> {
        let who = self
            .catalog
            .resolve_principal(principal, principal)
            .await
            .map_err(|e| unavailable(&e))?;

        let Some(asset) = self
            .catalog
            .get_asset_by_fqn(fqn)
            .await
            .map_err(|e| unavailable(&e))?
        else {
            return Ok(None);
        };
        // **The same visibility gate the context tool uses**, and reused rather
        // than restated: two implementations of "may this principal see it" is one
        // more than can be kept in step, and knowledge about an asset is at least
        // as sensitive as the asset.
        if self.catalog.get_asset_for(&who, asset.id).await.is_err() {
            return Ok(None);
        }

        // Current memories only. A superseded memory reaching an agent as an
        // unmarked peer of its own correction is the worst possible outcome here —
        // it is not stale, it is *withdrawn*, and there is no flag that makes
        // presenting both defensible.
        let recalled = self
            .catalog
            .recall(asset.id, query, false)
            .await
            .map_err(|e| unavailable(&e))?;

        // Read once for the whole set rather than per memory: they are all about
        // the same asset, so the queue is the same queue.
        let conflicts = self
            .catalog
            .contradictions_about(asset.id)
            .await
            .map_err(|e| unavailable(&e))?;
        let disputed: std::collections::HashSet<uuid::Uuid> = conflicts
            .iter()
            .flat_map(|conflict| [conflict.a, conflict.b])
            .collect();

        Ok(Some(
            recalled
                .into_iter()
                .map(|item| MemoryContext {
                    kind: format!("{:?}", item.memory.kind).to_lowercase(),
                    content: item.memory.content.clone(),
                    summary: item.memory.summary.clone(),
                    confidence: item.memory.confidence,
                    human_authored: matches!(
                        item.memory.authorship,
                        graph_owl_core::memory::Authorship::Human { .. }
                    ),
                    staleness: staleness_note(&item.staleness),
                    contradicted: disputed.contains(&item.memory.id),
                })
                .collect(),
        ))
    }
}

/// Staleness as a sentence an agent can pass on, or `None` when fresh.
///
/// Words rather than a code, because this text is going into a language model's
/// context and a bare `possiblyStale` gives it nothing to tell a reader. `None`
/// for fresh, so the flag is present exactly when it means something — a field
/// that is always set is a field that gets summarised away.
fn staleness_note(staleness: &graph_owl_core::memory::Staleness) -> Option<String> {
    use graph_owl_core::memory::Staleness;
    match staleness {
        Staleness::Fresh => None,
        Staleness::PossiblyStale { since } => Some(format!(
            "the asset has changed since this was written (now version {}.{}), \
             but only in a backward-compatible way",
            since.major, since.minor
        )),
        Staleness::Stale { since } => Some(format!(
            "the asset has changed in a breaking way since this was written \
             (now version {}.{}); what it describes may no longer exist",
            since.major, since.minor
        )),
        Staleness::SubjectUnknown => Some(
            "the asset this describes could not be resolved, so this could \
                  not be checked against it"
                .to_string(),
        ),
    }
}
