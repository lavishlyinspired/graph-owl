//! The real `Catalog` over HTTP — Epic 20, decision 6.
//!
//! Blocking on purpose: the CLI does one bounded sequence of requests and
//! exits, so an async runtime would add a dependency and a `#[tokio::main]`
//! to buy concurrency nobody uses. Apply is sequential by necessity anyway —
//! a child cannot be sent before its parent's id comes back.

use crate::client::{Catalog, ClientError, UpsertRequest};
use crate::plan::LiveEntity;

/// How many assets one page of the scope read asks for.
///
/// Not a tuning number: `01-api-conventions.md` caps a page, and asking for
/// more than the server will give simply wastes the round trip. Paging
/// continues until the catalog stops handing back a cursor, so this only
/// decides *how many* round trips, never how much is seen.
const PAGE_SIZE: usize = 200;

pub struct HttpCatalog {
    base_url: String,
    token: Option<String>,
    client: reqwest::blocking::Client,
}

impl HttpCatalog {
    /// # Errors
    ///
    /// [`ClientError::Transport`] if the HTTP client cannot be built.
    pub fn new(base_url: impl Into<String>, token: Option<String>) -> Result<Self, ClientError> {
        Ok(Self {
            // Trailing slashes are stripped so `--server http://x/` and
            // `--server http://x` cannot produce different URLs — a
            // difference nobody would expect to matter and everybody would
            // eventually hit.
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token,
            client: reqwest::blocking::Client::builder()
                .build()
                .map_err(|e| ClientError::Transport(e.to_string()))?,
        })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::blocking::RequestBuilder {
        let builder = self
            .client
            .request(method, format!("{}{path}", self.base_url));
        match &self.token {
            Some(token) => builder.bearer_auth(token),
            None => builder,
        }
    }

    /// Sends, and turns anything that is not a success into a `Refused`
    /// carrying **the catalog's own message**. The server is the one that
    /// knows why a write was rejected, and re-wording it here would lose the
    /// detail that makes it fixable.
    fn send(
        &self,
        request: reqwest::blocking::RequestBuilder,
    ) -> Result<serde_json::Value, ClientError> {
        let response = request
            .send()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        let status = response.status();
        let body: serde_json::Value = response.json().unwrap_or(serde_json::Value::Null);
        if status.is_success() {
            Ok(body)
        } else {
            Err(ClientError::Refused {
                status: status.as_u16(),
                detail: body
                    .get("detail")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&body.to_string())
                    .to_string(),
            })
        }
    }
}

fn entity_from_json(asset: &serde_json::Value) -> Option<LiveEntity> {
    Some(LiveEntity {
        id: asset["id"].as_str()?.to_string(),
        fully_qualified_name: asset["fullyQualifiedName"].as_str()?.to_string(),
        kind: asset["kind"].as_str()?.to_string(),
        description: asset["description"].as_str().map(ToString::to_string),
    })
}

impl Catalog for HttpCatalog {
    fn live_within(&self, scope_prefixes: &[String]) -> Result<Vec<LiveEntity>, ClientError> {
        let mut all = Vec::new();
        let mut after: Option<String> = None;

        // **Paged to exhaustion.** Stopping at one page would silently plan a
        // prune for every entity past it — the catalog would look emptier
        // than it is, and decision 2's scope guard cannot save you from a
        // read that lied.
        loop {
            let mut path = format!("/assets?limit={PAGE_SIZE}");
            if let Some(cursor) = &after {
                path.push_str(&format!("&after={cursor}"));
            }
            let body = self.send(self.request(reqwest::Method::GET, &path))?;

            let page: Vec<LiveEntity> = body["data"]
                .as_array()
                .map(|assets| assets.iter().filter_map(entity_from_json).collect())
                .unwrap_or_default();
            let exhausted = page.len() < PAGE_SIZE;
            all.extend(page);

            after = body["nextCursor"].as_str().map(ToString::to_string);
            if exhausted || after.is_none() {
                break;
            }
        }

        // Filtering here is a second line, not the first: the scope is what
        // decision 2 makes authoritative, and everything downstream treats
        // "absent from this list" as "prune me".
        all.retain(|entity| {
            scope_prefixes.iter().any(|prefix| {
                entity.fully_qualified_name == *prefix
                    || entity
                        .fully_qualified_name
                        .starts_with(&format!("{prefix}."))
            })
        });
        Ok(all)
    }

    fn upsert(&self, entity: &UpsertRequest) -> Result<String, ClientError> {
        // **Built key by key so an undeclared field is an absent key, never
        // a null.** Decision 4 is enforced right here, on the wire: a
        // serialized struct with `Option` fields would emit `"description":
        // null` and reset every hand-curated description on first apply.
        let mut payload = serde_json::Map::new();
        payload.insert("kind".into(), entity.kind.clone().into());
        payload.insert("name".into(), entity.name.clone().into());
        if let Some(parent_id) = &entity.parent_id {
            payload.insert("parentId".into(), parent_id.clone().into());
        }
        if let Some(description) = &entity.description {
            payload.insert("description".into(), description.clone().into());
        }

        let body = self.send(
            self.request(reqwest::Method::POST, "/assets")
                .json(&serde_json::Value::Object(payload)),
        )?;
        body["id"]
            .as_str()
            .map(ToString::to_string)
            .ok_or_else(|| ClientError::Transport("the catalog returned no id".to_string()))
    }

    fn tombstone(&self, fully_qualified_name: &str) -> Result<(), ClientError> {
        // Delete addresses an id, so the FQN is resolved first. One extra
        // round trip per prune, which is the right trade for an operation
        // this irreversible: resolving locally from a stale list would risk
        // deleting whatever now holds that name.
        let found = self.live_within(&[fully_qualified_name.to_string()])?;
        let target = found
            .iter()
            .find(|entity| entity.fully_qualified_name == fully_qualified_name)
            .ok_or_else(|| ClientError::Refused {
                status: 404,
                detail: format!("`{fully_qualified_name}` no longer exists"),
            })?;
        self.send(self.request(reqwest::Method::DELETE, &format!("/assets/{}", target.id)))?;
        Ok(())
    }
}
