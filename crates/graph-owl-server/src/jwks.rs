use std::time::Instant;

use base64::Engine as _;
use serde::Deserialize;
use tokio::sync::RwLock;

const CACHE_TTL: std::time::Duration = std::time::Duration::from_hours(1);
const JWKS_URI: &str = ".well-known/jwks.json";

/// Fetches and caches JWKS from an OIDC issuer.
///
/// Thread-safe and request-safe: the cache is behind `Arc<RwLock<>>`, and
/// the HTTP client is `reqwest`'s own connection pool.
///
/// Cold start fetches synchronously on the first request. Cache refreshes
/// happen in-band too — a background refresh loop would add a timer and
/// complexity for negligible gain given the 1-hour TTL.
pub struct JwksClient {
    issuer: String,
    audience: String,
    cache: RwLock<JwksCache>,
    http: reqwest::Client,
}

/// The shortest interval between two fetches of the same JWKS.
///
/// **An unknown `kid` triggers a refresh, and `kid` is attacker-controlled.**
/// Without a floor, a stream of tokens carrying random key ids becomes one
/// outbound request to the identity provider per inbound request — a denial of
/// service pointed at the `IdP`, amplified by us, and a self-inflicted latency
/// spike while every request waits on a network round trip.
///
/// 60 seconds: long enough that a flood costs at most one fetch a minute, short
/// enough that a genuine key rotation is picked up within a minute. The upper
/// bound on delay matters because it is the window in which valid tokens signed
/// by a new key are rejected, and providers publish a new key well before they
/// sign with it — so a minute is comfortably inside the overlap.
const MIN_REFETCH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

struct JwksCache {
    keys: Vec<Jwk>,
    /// `None` until the first successful fetch. Distinct from "fetched long
    /// ago": an empty cache that has never been filled must not look fresh,
    /// which is what `Instant::now()` at construction made it look like.
    fetched_at: Option<Instant>,
    /// When a fetch was last *attempted*, successful or not. Separate from
    /// `fetched_at` because the rate limit has to bound failures too — a
    /// provider that is down is exactly when retrying every request is worst.
    attempted_at: Option<Instant>,
}

/// Whether a cache of this age must be refetched before use.
///
/// Takes the age rather than reading the clock, so the boundary is decidable: a
/// function that calls `Instant::elapsed()` itself can never be handed *exactly*
/// the TTL, which leaves `>` and `>=` indistinguishable to any test and the
/// choice between them unrecorded.
///
/// At exactly the TTL a key is **stale**. Between refetching a moment early and
/// serving a key a moment past its stated lifetime, the first costs one HTTP
/// request and the second is the behaviour the TTL exists to prevent.
///
/// `None` — never fetched — is stale. An empty cache that has never been filled
/// must not report itself as fresh.
fn is_expired(age: Option<std::time::Duration>) -> bool {
    age.is_none_or(|age| age >= CACHE_TTL)
}

/// Whether enough time has passed since the last *attempt* to try again.
///
/// At exactly the interval a refetch is allowed: the limit is a floor on the
/// gap between attempts, and refusing at the boundary would make the effective
/// floor unbounded above by a hair.
fn may_attempt(since_last: Option<std::time::Duration>) -> bool {
    since_last.is_none_or(|since| since >= MIN_REFETCH_INTERVAL)
}

impl JwksCache {
    fn is_stale(&self) -> bool {
        is_expired(self.fetched_at.map(|fetched| fetched.elapsed()))
    }

    fn may_refetch(&self) -> bool {
        may_attempt(self.attempted_at.map(|attempted| attempted.elapsed()))
    }

    fn key_by_kid(&self, kid: &str) -> Option<&Jwk> {
        self.keys.iter().find(|k| k.kid == kid)
    }
}

/// Keys arrive as raw JSON and are narrowed afterwards.
///
/// **Deliberately not `Vec<Jwk>`.** A JWKS is a heterogeneous set: RFC 7517
/// allows EC and OKP keys beside RSA ones, `alg` is optional on every key, and
/// a provider may add a key type this build has never heard of. Deserializing
/// straight into a struct that requires `n` and `e` makes *one* unrecognised
/// sibling fail the whole document — so a tenant that adds an EC key loses
/// every RSA key too, and authentication stops for a reason nothing in the
/// error mentions.
///
/// Parsing loosely and filtering after is what keeps an unusable key merely
/// unusable.
#[derive(Deserialize)]
struct JwksResponse {
    keys: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Jwk {
    kid: String,
    n: String,
    e: String,
}

/// The RSA signing keys in a JWKS document, in the order the provider listed
/// them.
///
/// A key is kept when it is RSA, carries a modulus and exponent, and is not
/// declared for something other than RS256 signing:
///
/// - **`alg` absent is kept.** It is optional in RFC 7517 and several providers
///   omit it; requiring it would reject valid keys. `alg` present and not
///   `RS256` is dropped, because the provider has said what that key is for.
/// - **`use` absent is kept**, for the same reason. `use: "enc"` is dropped —
///   an encryption key is not a signature key, and verifying with one would
///   fail confusingly rather than safely.
/// - **A key with no `kid` is dropped.** Lookup is by `kid`, and a key that
///   cannot be selected cannot be used; keeping it would only let it match the
///   empty string.
///
/// The verifier pins RS256 independently (`verify_jwks`), so this filter is
/// defence in depth rather than the only thing standing between a token and a
/// confused-algorithm attack.
fn rsa_signing_keys(document: &[serde_json::Value]) -> Vec<Jwk> {
    document
        .iter()
        .filter(|key| key.get("kty").and_then(serde_json::Value::as_str) == Some("RSA"))
        .filter(|key| {
            !matches!(
                key.get("alg").and_then(serde_json::Value::as_str),
                Some(alg) if alg != "RS256"
            )
        })
        .filter(|key| {
            !matches!(
                key.get("use").and_then(serde_json::Value::as_str),
                Some(purpose) if purpose != "sig"
            )
        })
        .filter_map(|key| {
            Some(Jwk {
                kid: key.get("kid")?.as_str()?.to_string(),
                n: key.get("n")?.as_str()?.to_string(),
                e: key.get("e")?.as_str()?.to_string(),
            })
        })
        .filter(|jwk| !jwk.kid.is_empty() && !jwk.n.is_empty() && !jwk.e.is_empty())
        .collect()
}

impl JwksClient {
    /// Create a new client for the given OIDC issuer and expected audience.
    ///
    /// The issuer is used to construct the JWKS URI and to validate the `iss`
    /// claim. The audience is used to validate `aud`.
    pub fn new(issuer: impl Into<String>, audience: impl Into<String>) -> Self {
        Self {
            issuer: issuer.into(),
            audience: audience.into(),
            cache: RwLock::new(JwksCache {
                keys: Vec::new(),
                fetched_at: None,
                attempted_at: None,
            }),
            http: reqwest::Client::new(),
        }
    }

    /// The issuer URL, with `iss` claim validation as an exact match.
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// The expected audience for `aud` claim validation.
    pub fn audience(&self) -> &str {
        &self.audience
    }

    /// Resolve a `DecodingKey` for the given KID.
    ///
    /// Returns cached key if fresh. On stale cache or unknown KID, fetches
    /// the JWKS endpoint and retries — an unknown KID from a valid issuer
    /// means the key was rotated, and a single fetch resolves it.
    #[allow(clippy::missing_errors_doc)]
    pub async fn decoding_key(&self, kid: &str) -> Result<jsonwebtoken::DecodingKey, JwksError> {
        {
            let cache = self.cache.read().await;
            if !cache.is_stale()
                && let Some(jwk) = cache.key_by_kid(kid)
            {
                return Self::build_key(jwk);
            }
            // Rate-limited, because `kid` comes from the token and therefore
            // from whoever sent it. Answering "unknown key" from the cache is
            // the correct response to a key we have recently looked for and
            // not found; going back to the provider for each one turns a flood
            // of forged tokens into a flood of outbound requests.
            if !cache.may_refetch() {
                return Err(JwksError::UnknownKey(kid.to_string()));
            }
        }

        // Stale or unknown KID: refresh and retry once.
        self.fetch_keys().await?;

        let cache = self.cache.read().await;
        match cache.key_by_kid(kid) {
            Some(jwk) => Self::build_key(jwk),
            None => Err(JwksError::UnknownKey(kid.to_string())),
        }
    }

    fn jwks_url(&self) -> String {
        let issuer = self.issuer.trim_end_matches('/');
        format!("{issuer}/{JWKS_URI}")
    }

    async fn fetch_keys(&self) -> Result<(), JwksError> {
        // Stamped before the request, so a provider that is down does not get
        // retried on every request while it is down.
        self.cache.write().await.attempted_at = Some(Instant::now());

        let url = self.jwks_url();
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| JwksError::FetchFailed(url.clone(), e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            return Err(JwksError::FetchFailed(url, format!("HTTP {status}")));
        }

        let body: JwksResponse = response
            .json()
            .await
            .map_err(|e| JwksError::ParseFailed(e.to_string()))?;

        let keys = rsa_signing_keys(&body.keys);
        if keys.is_empty() {
            return Err(JwksError::NoUsableKeys);
        }

        let mut cache = self.cache.write().await;
        cache.keys = keys;
        cache.fetched_at = Some(Instant::now());
        Ok(())
    }

    fn build_key(jwk: &Jwk) -> Result<jsonwebtoken::DecodingKey, JwksError> {
        let modulus = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&jwk.n)
            .map_err(|e| JwksError::KeyFormat(format!("n: {e}")))?;
        let exponent = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&jwk.e)
            .map_err(|e| JwksError::KeyFormat(format!("e: {e}")))?;
        Ok(jsonwebtoken::DecodingKey::from_rsa_raw_components(
            &modulus, &exponent,
        ))
    }
}

#[derive(Debug)]
pub enum JwksError {
    FetchFailed(String, String),
    ParseFailed(String),
    UnknownKey(String),
    NoUsableKeys,
    KeyFormat(String),
}

impl std::fmt::Display for JwksError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JwksError::FetchFailed(url, detail) => {
                write!(f, "failed to fetch JWKS from {url}: {detail}")
            }
            JwksError::ParseFailed(detail) => write!(f, "failed to parse JWKS response: {detail}"),
            JwksError::UnknownKey(kid) => write!(f, "unknown KID: {kid}"),
            JwksError::NoUsableKeys => write!(f, "no usable RSA/RS256 keys in JWKS"),
            JwksError::KeyFormat(detail) => write!(f, "key format error: {detail}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rsa(kid: &str) -> serde_json::Value {
        json!({ "kty": "RSA", "alg": "RS256", "use": "sig", "kid": kid,
                "n": "0vx7ag", "e": "AQAB" })
    }

    fn keys(document: &[serde_json::Value]) -> Vec<String> {
        rsa_signing_keys(document)
            .into_iter()
            .map(|jwk| jwk.kid)
            .collect()
    }

    mod one_unusable_key_does_not_cost_the_others {
        use super::*;

        /// **The outage this parsing shape exists to prevent.** An EC key has
        /// no `n` or `e`; a struct that requires them fails the whole
        /// document, so a tenant that adds one loses every RSA key too — and
        /// authentication stops for a reason nothing in the error mentions.
        #[test]
        fn an_elliptic_curve_key_beside_an_rsa_one_leaves_the_rsa_one_usable() {
            let document = vec![
                json!({ "kty": "EC", "crv": "P-256", "kid": "ec-1", "x": "f83", "y": "x_FE" }),
                rsa("rsa-1"),
            ];

            assert_eq!(keys(&document), vec!["rsa-1"]);
        }

        #[test]
        fn a_key_of_an_unknown_type_is_ignored_rather_than_fatal() {
            let document = vec![
                json!({ "kty": "OKP", "crv": "Ed25519", "kid": "x" }),
                rsa("r"),
            ];

            assert_eq!(keys(&document), vec!["r"]);
        }

        #[test]
        fn an_rsa_key_missing_its_modulus_is_dropped_and_the_rest_survive() {
            let document = vec![
                json!({ "kty": "RSA", "alg": "RS256", "kid": "broken", "e": "AQAB" }),
                rsa("good"),
            ];

            assert_eq!(keys(&document), vec!["good"]);
        }

        #[test]
        fn garbage_in_the_array_is_skipped() {
            assert_eq!(
                keys(&[json!("not an object"), json!(7), rsa("r")]),
                vec!["r"]
            );
        }
    }

    mod which_keys_are_usable {
        use super::*;

        /// `alg` is optional in RFC 7517 and several providers omit it.
        /// Requiring it would reject valid keys.
        #[test]
        fn a_key_without_an_alg_is_kept() {
            let document = vec![json!({ "kty": "RSA", "kid": "k", "n": "0vx", "e": "AQAB" })];

            assert_eq!(keys(&document), vec!["k"]);
        }

        /// But a key the provider declared for something else is not ours to
        /// reinterpret.
        #[test]
        fn a_key_declared_for_another_algorithm_is_dropped() {
            for alg in ["RS512", "ES256", "HS256", "none"] {
                let document =
                    vec![json!({ "kty": "RSA", "alg": alg, "kid": "k", "n": "0", "e": "A" })];

                assert!(keys(&document).is_empty(), "{alg} should not be usable");
            }
        }

        #[test]
        fn an_encryption_key_is_not_a_signature_key() {
            let document =
                vec![json!({ "kty": "RSA", "use": "enc", "kid": "k", "n": "0", "e": "A" })];

            assert!(keys(&document).is_empty());
        }

        #[test]
        fn a_key_without_a_declared_use_is_kept() {
            let document = vec![json!({ "kty": "RSA", "kid": "k", "n": "0", "e": "A" })];

            assert_eq!(keys(&document), vec!["k"]);
        }

        /// Lookup is by `kid`. A key that cannot be selected cannot be used,
        /// and keeping it would only let it match the empty string.
        #[test]
        fn a_key_with_no_kid_is_dropped() {
            for document in [
                vec![json!({ "kty": "RSA", "n": "0", "e": "A" })],
                vec![json!({ "kty": "RSA", "kid": "", "n": "0", "e": "A" })],
            ] {
                assert!(keys(&document).is_empty());
            }
        }

        #[test]
        fn a_non_rsa_key_is_never_usable_however_it_is_labelled() {
            let document =
                vec![json!({ "kty": "oct", "alg": "RS256", "kid": "k", "n": "0", "e": "A" })];

            assert!(keys(&document).is_empty());
        }

        /// The negative that stops the filter being satisfied by "drop
        /// everything": an ordinary provider document keeps every key.
        #[test]
        fn an_ordinary_rotation_pair_keeps_both_keys_in_order() {
            assert_eq!(
                keys(&[rsa("current"), rsa("next")]),
                vec!["current", "next"]
            );
        }
    }

    mod the_cache_answers_honestly_about_its_own_age {
        use super::*;

        fn cache(fetched_at: Option<Instant>, attempted_at: Option<Instant>) -> JwksCache {
            JwksCache {
                keys: Vec::new(),
                fetched_at,
                attempted_at,
            }
        }

        /// A cache that has never been filled must not look fresh. Stamping
        /// `Instant::now()` at construction made an empty cache report itself
        /// as an hour from stale.
        #[test]
        fn a_cache_that_has_never_fetched_is_stale() {
            assert!(cache(None, None).is_stale());
        }

        #[test]
        fn a_cache_filled_just_now_is_fresh() {
            assert!(!cache(Some(Instant::now()), None).is_stale());
        }

        #[test]
        fn a_cache_older_than_the_ttl_is_stale() {
            let long_ago = Instant::now()
                .checked_sub(CACHE_TTL + std::time::Duration::from_secs(1))
                .expect("an instant an hour ago");

            assert!(cache(Some(long_ago), None).is_stale());
        }

        /// `kid` comes from the token, so an unknown one is attacker-chosen. A
        /// flood of them must not become one outbound request each.
        #[test]
        fn a_refetch_is_refused_inside_the_rate_limit_window() {
            assert!(!cache(None, Some(Instant::now())).may_refetch());
        }

        #[test]
        fn a_refetch_is_allowed_once_the_window_has_passed() {
            let earlier = Instant::now()
                .checked_sub(MIN_REFETCH_INTERVAL + std::time::Duration::from_secs(1))
                .expect("an instant a minute ago");

            assert!(cache(None, Some(earlier)).may_refetch());
        }

        /// And the negative: the very first request must be allowed through,
        /// or a rate limit that starts closed means the server never fetches
        /// keys at all and every token fails.
        #[test]
        fn the_first_ever_fetch_is_allowed() {
            assert!(cache(None, None).may_refetch());
        }
    }

    mod the_freshness_boundary_is_decided_rather_than_left_to_the_clock {
        use super::*;
        use std::time::Duration;

        /// A cache that has never been filled must not report itself fresh.
        #[test]
        fn never_fetched_is_expired() {
            assert!(is_expired(None));
        }

        #[test]
        fn just_fetched_is_fresh() {
            assert!(!is_expired(Some(Duration::ZERO)));
        }

        #[test]
        fn a_moment_before_the_ttl_is_still_fresh() {
            assert!(!is_expired(Some(CACHE_TTL - Duration::from_nanos(1))));
        }

        /// **The boundary, stated rather than inherited.** Between refetching a
        /// moment early and serving a key a moment past its stated lifetime,
        /// the first costs one HTTP request and the second is what the TTL
        /// exists to prevent.
        #[test]
        fn exactly_the_ttl_is_expired() {
            assert!(is_expired(Some(CACHE_TTL)));
        }

        #[test]
        fn past_the_ttl_is_expired() {
            assert!(is_expired(Some(CACHE_TTL + Duration::from_secs(1))));
        }

        #[test]
        fn a_first_attempt_is_always_allowed() {
            assert!(may_attempt(None));
        }

        #[test]
        fn an_attempt_inside_the_window_is_refused() {
            assert!(!may_attempt(Some(Duration::ZERO)));
            assert!(!may_attempt(Some(
                MIN_REFETCH_INTERVAL - Duration::from_nanos(1)
            )));
        }

        /// The other boundary, and it goes the other way: the limit is a floor
        /// on the gap between attempts, so refusing *at* the floor would make
        /// the effective floor unbounded above by a hair.
        #[test]
        fn an_attempt_exactly_at_the_interval_is_allowed() {
            assert!(may_attempt(Some(MIN_REFETCH_INTERVAL)));
        }
    }

    mod the_jwks_url {
        use super::*;

        #[test]
        fn is_the_issuer_plus_the_well_known_path() {
            let client = JwksClient::new("https://tenant.us.auth0.com", "aud");

            assert_eq!(
                client.jwks_url(),
                "https://tenant.us.auth0.com/.well-known/jwks.json"
            );
        }

        /// Issuers are commonly configured with a trailing slash — Auth0's own
        /// `iss` claim carries one — and a doubled slash is a 404 from some
        /// providers, which surfaces as "authentication is broken".
        #[test]
        fn tolerates_a_trailing_slash_on_the_issuer() {
            let client = JwksClient::new("https://tenant.us.auth0.com/", "aud");

            assert_eq!(
                client.jwks_url(),
                "https://tenant.us.auth0.com/.well-known/jwks.json"
            );
        }

        #[test]
        fn keeps_a_path_prefixed_issuer_intact() {
            let client = JwksClient::new("https://example.com/oauth2/default", "aud");

            assert_eq!(
                client.jwks_url(),
                "https://example.com/oauth2/default/.well-known/jwks.json"
            );
        }
    }
}
