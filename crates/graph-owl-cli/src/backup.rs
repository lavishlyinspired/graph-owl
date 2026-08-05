//! Lossless catalog backup and restore — Epic 37b.
//!
//! **Distinct from `export.rs`'s declarative export** (Epic 20, deliberately
//! lossy): that command reads live state through the ordinary `Catalog`
//! trait and re-derives declarations. This one moves an opaque archive's raw
//! bytes to and from `/admin/export` and `/admin/restore`, never parsing the
//! archive itself client-side — the format is `graph-owl-core::archive`'s
//! concern, not this crate's.
//!
//! Blocking `reqwest`, matching `http.rs`'s own reasoning: one bounded
//! sequence of requests, no concurrency to buy with an async runtime.

use std::path::Path;

/// `domain:x` / `service:x` / `entity-type:x` → the JSON shape
/// `graph_owl_core::archive::ScopeSelector` deserializes — an adjacently
/// tagged enum (`{"type": ..., "value": ...}`), camelCase per this
/// project's wire convention. Domain and service scoping both become an
/// FQN-prefix selector (decision 5: "domain and service scoping is
/// FQN-prefix based"); entity-type becomes a kind selector.
///
/// # Errors
/// A string not shaped `word:value`, or a `word` that names none of the
/// three recognised scope kinds.
pub fn parse_scope_arg(raw: &str) -> Result<serde_json::Value, String> {
    let (kind, value) = raw
        .split_once(':')
        .ok_or_else(|| format!("`{raw}` is not `domain:x`, `service:x`, or `entity-type:x`"))?;
    if value.is_empty() {
        return Err(format!("`{raw}` names no value after the `:`"));
    }
    match kind {
        "entity-type" => Ok(serde_json::json!({"type": "kind", "value": value})),
        "domain" | "service" => Ok(serde_json::json!({"type": "fqnPrefix", "value": value})),
        other => Err(format!(
            "`{other}` is not a scope kind; expected domain, service, or entity-type"
        )),
    }
}

/// Streams the whole catalog (or a scoped, redacted slice) to `out`.
///
/// # Errors
/// A malformed `--scope` argument, a transport failure, a non-success HTTP
/// status (the server's own detail is passed through), or a failure writing
/// `out`.
pub fn backup(
    server: &str,
    token: Option<&str>,
    out: &Path,
    scope: &[String],
    redact: &[String],
) -> Result<(), String> {
    let scope_json = if scope.is_empty() {
        None
    } else {
        Some(
            scope
                .iter()
                .map(|s| parse_scope_arg(s))
                .collect::<Result<Vec<_>, _>>()?,
        )
    };
    let body = serde_json::json!({ "scope": scope_json, "redact": redact });

    let client = reqwest::blocking::Client::new();
    let mut request = client
        .post(format!("{}/admin/export", server.trim_end_matches('/')))
        .json(&body);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().map_err(|e| e.to_string())?;
    let status = response.status();
    if !status.is_success() {
        let detail = response.text().unwrap_or_default();
        return Err(format!("export refused: HTTP {status}: {detail}"));
    }
    let bytes = response.bytes().map_err(|e| e.to_string())?;
    std::fs::write(out, &bytes).map_err(|e| e.to_string())
}

/// Restores an archive `backup` produced.
///
/// # Errors
/// A failure reading `input`, a transport failure, or a non-success HTTP
/// status — including the archive being refused for a checksum mismatch, a
/// truncation, a newer format version, or (under `fail`) a conflict.
pub fn restore(
    server: &str,
    token: Option<&str>,
    input: &Path,
    conflict_policy: &str,
    regenerate_ids: bool,
) -> Result<serde_json::Value, String> {
    let bytes = std::fs::read(input).map_err(|e| e.to_string())?;

    let client = reqwest::blocking::Client::new();
    let mut request = client
        .post(format!(
            "{}/admin/restore?conflictPolicy={conflict_policy}&regenerateIds={regenerate_ids}",
            server.trim_end_matches('/')
        ))
        .body(bytes);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().map_err(|e| e.to_string())?;
    let status = response.status();
    let body: serde_json::Value = response.json().unwrap_or(serde_json::Value::Null);
    if !status.is_success() {
        return Err(format!("restore refused: HTTP {status}: {body}"));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_type_parses_as_a_kind_selector() {
        assert_eq!(
            parse_scope_arg("entity-type:table").expect("parses"),
            serde_json::json!({"type": "kind", "value": "table"})
        );
    }

    #[test]
    fn domain_and_service_both_parse_as_an_fqn_prefix_selector() {
        assert_eq!(
            parse_scope_arg("domain:payments").expect("parses"),
            serde_json::json!({"type": "fqnPrefix", "value": "payments"})
        );
        assert_eq!(
            parse_scope_arg("service:snowflake_prod").expect("parses"),
            serde_json::json!({"type": "fqnPrefix", "value": "snowflake_prod"})
        );
    }

    #[test]
    fn a_scope_arg_with_no_colon_is_refused() {
        assert!(parse_scope_arg("payments").is_err());
    }

    #[test]
    fn an_unknown_scope_kind_is_refused() {
        assert!(parse_scope_arg("region:us-east").is_err());
    }

    #[test]
    fn a_scope_arg_with_no_value_is_refused() {
        assert!(parse_scope_arg("domain:").is_err());
    }
}
