//! Embeddings, generated out of process — this crate's own module doc
//! (`lib.rs`) names the process boundary this module fills: "the port that
//! matters is the one at that process boundary, and it does not exist yet
//! either." It exists now, for Epic 31's own semantic ranking term
//! (`plans/31-memory.md`, `graph_owl_core::recall::Candidate::semantic`).
//!
//! **Provider-agnostic by construction, not by claim.** [`EmbeddingClient`]
//! speaks the OpenAI-compatible `/v1/embeddings` wire shape — the same
//! contract `OpenAI` itself, a self-hosted `LiteLLM` proxy, and a self-hosted
//! `vLLM` instance all implement — so pointing it at a different backend is
//! [`EmbeddingConfig::base_url`], never a code change. This is the decision
//! the user made explicitly over a hardcoded single-vendor call.

use async_openai::{
    Client,
    config::OpenAIConfig,
    types::embeddings::{CreateEmbeddingRequestArgs, EmbeddingInput},
};

/// Where to call, and with what credentials.
///
/// `api_key` is `Option` because not every OpenAI-compatible server
/// requires one — a local vLLM instance commonly does not.
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    /// e.g. `https://api.openai.com/v1`, `http://localhost:4000/v1`
    /// (`LiteLLM`), or `http://localhost:8000/v1` (`vLLM`).
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
}

#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    #[error("embedding request failed: {0}")]
    Request(String),
    #[error("embedding response carried no vector")]
    EmptyResponse,
}

/// Calls a configured OpenAI-compatible `/v1/embeddings` endpoint for one
/// piece of text at a time.
pub struct EmbeddingClient {
    client: Client<OpenAIConfig>,
    model: String,
}

impl EmbeddingClient {
    #[must_use]
    pub fn new(config: EmbeddingConfig) -> Self {
        let mut openai_config = OpenAIConfig::new().with_api_base(config.base_url);
        if let Some(key) = config.api_key {
            openai_config = openai_config.with_api_key(key);
        }
        Self {
            client: Client::with_config(openai_config),
            model: config.model,
        }
    }

    /// # Errors
    ///
    /// `Request` if the call itself fails (network, non-2xx, malformed
    /// body). `EmptyResponse` if the server answers successfully but with
    /// no embedding at all — a malformed provider rather than a network
    /// failure, and worth distinguishing in a log line.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let request = CreateEmbeddingRequestArgs::default()
            .model(self.model.clone())
            .input(EmbeddingInput::String(text.to_string()))
            .build()
            .map_err(|error| EmbeddingError::Request(error.to_string()))?;

        let response = self
            .client
            .embeddings()
            .create(request)
            .await
            .map_err(|error| EmbeddingError::Request(error.to_string()))?;

        response
            .data
            .into_iter()
            .next()
            .map(|embedding| embedding.embedding)
            .ok_or(EmbeddingError::EmptyResponse)
    }
}

/// Cosine similarity, in `[-1, 1]`.
///
/// `0.0` — not a panic, not `NaN` — for mismatched lengths or a zero-length
/// input, and `0.0` again when either vector has zero magnitude. A real
/// embedding model never emits an all-zero vector, but the guard exists for
/// the same reason `graph_owl_core::recall::lexical_overlap` guards `0 / 0`:
/// an unguarded division here would poison [`graph_owl_core::recall::rank`]'s
/// sort, which only tolerates finite scores.
#[must_use]
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }

    let dot: f64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| f64::from(*x) * f64::from(*y))
        .sum();
    let magnitude_a: f64 = a.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    let magnitude_b: f64 = b.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();

    if magnitude_a == 0.0 || magnitude_b == 0.0 {
        return 0.0;
    }
    dot / (magnitude_a * magnitude_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    mod cosine_similarity_tests {
        use super::*;

        #[test]
        fn identical_vectors_score_one() {
            assert!((cosine_similarity(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]) - 1.0).abs() < 1e-9);
        }

        #[test]
        fn orthogonal_vectors_score_zero() {
            assert!((cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]) - 0.0).abs() < 1e-9);
        }

        #[test]
        fn opposite_vectors_score_negative_one() {
            assert!((cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]) - (-1.0)).abs() < 1e-9);
        }

        // The negative that proves the guard is a *length* check, not a
        // no-op: two same-length, differently-valued vectors must still be
        // compared for real, or a mutant collapsing the guard's `!=` to
        // always-false would slip through unnoticed.
        #[test]
        fn a_partial_match_scores_strictly_between_zero_and_one() {
            let score = cosine_similarity(&[1.0, 1.0], &[1.0, 0.0]);
            assert!(score > 0.0 && score < 1.0, "got {score}");
        }

        #[test]
        fn mismatched_lengths_score_zero_rather_than_panicking() {
            assert!((cosine_similarity(&[1.0, 2.0], &[1.0]) - 0.0).abs() < 1e-9);
        }

        #[test]
        fn empty_vectors_score_zero_rather_than_nan() {
            let score = cosine_similarity(&[], &[]);
            assert!(!score.is_nan());
            assert!((score - 0.0).abs() < 1e-9);
        }

        #[test]
        fn a_zero_magnitude_vector_scores_zero_rather_than_nan() {
            let score = cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]);
            assert!(!score.is_nan());
            assert!((score - 0.0).abs() < 1e-9);
        }
    }

    mod embedding_client_tests {
        use super::*;

        /// A real local `/embeddings` endpoint — this project's established
        /// pattern (`graph-owl-api`'s `start_mock_sparql_endpoint`) for
        /// proving an HTTP client against a server that actually answers,
        /// rather than a placeholder.
        async fn spawn_mock_embeddings_endpoint(
            status: axum::http::StatusCode,
            body: &'static str,
        ) -> (String, tokio::task::JoinHandle<()>) {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind a free loopback port");
            let addr = listener.local_addr().expect("local addr");
            let router = axum::Router::new().route(
                "/embeddings",
                axum::routing::post(move || async move {
                    (
                        status,
                        [(axum::http::header::CONTENT_TYPE, "application/json")],
                        body,
                    )
                }),
            );
            let handle = tokio::spawn(async move {
                axum::serve(listener, router)
                    .await
                    .expect("mock embeddings endpoint");
            });
            (format!("http://{addr}"), handle)
        }

        fn config(base_url: String) -> EmbeddingConfig {
            EmbeddingConfig {
                base_url,
                api_key: None,
                model: "test-embedding-model".to_string(),
            }
        }

        const WELL_FORMED_RESPONSE: &str = r#"{
            "object": "list",
            "model": "test-embedding-model",
            "data": [
                { "index": 0, "object": "embedding", "embedding": [0.1, 0.2, 0.3] }
            ],
            "usage": { "prompt_tokens": 3, "total_tokens": 3 }
        }"#;

        #[tokio::test]
        async fn a_well_formed_response_returns_the_vector() {
            let (base_url, _handle) =
                spawn_mock_embeddings_endpoint(axum::http::StatusCode::OK, WELL_FORMED_RESPONSE)
                    .await;
            let client = EmbeddingClient::new(config(base_url));

            let embedding = client.embed("upi_transactions").await.expect("embed");

            assert_eq!(embedding, vec![0.1, 0.2, 0.3]);
        }

        #[tokio::test]
        async fn an_error_response_is_reported_as_a_request_error() {
            let (base_url, _handle) = spawn_mock_embeddings_endpoint(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                r#"{"error": {"message": "boom"}}"#,
            )
            .await;
            let client = EmbeddingClient::new(config(base_url));

            let result = client.embed("upi_transactions").await;

            assert!(matches!(result, Err(EmbeddingError::Request(_))));
        }

        #[tokio::test]
        async fn an_empty_data_array_is_reported_as_empty_response() {
            let (base_url, _handle) = spawn_mock_embeddings_endpoint(
                axum::http::StatusCode::OK,
                r#"{
                    "object": "list",
                    "model": "test-embedding-model",
                    "data": [],
                    "usage": { "prompt_tokens": 3, "total_tokens": 3 }
                }"#,
            )
            .await;
            let client = EmbeddingClient::new(config(base_url));

            let result = client.embed("upi_transactions").await;

            assert!(matches!(result, Err(EmbeddingError::EmptyResponse)));
        }
    }
}
