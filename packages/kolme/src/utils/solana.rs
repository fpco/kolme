//! Solana-specific helpers.

use solana_rpc_client_api::client_error;

/// Helper function to redact and wrap Solana RPC errors to hide sensitive information
pub fn redact_solana_error(mut error: client_error::Error) -> client_error::Error {
    if let client_error::ErrorKind::Reqwest(mut reqwest_error) = error.kind {
        let url = reqwest_error.url_mut();
        if let Some(url) = url {
            url.query_pairs_mut().clear();
        }
        error.kind = client_error::ErrorKind::Reqwest(reqwest_error);
    }
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::IntoFuture;

    #[tokio::test]
    async fn test_redact_solana_error() {
        const HTTP_SERVER_ADDR: &str = "127.0.0.1:3924";
        const FAIL_STATUS_PATH: &str = "/status/502";
        const SENSITIVE_VALUE: &str = "SENSITIVE";
        let fail_status_url = format!("http://{HTTP_SERVER_ADDR}{FAIL_STATUS_PATH}");

        // In order to construct a Reqwest error, we need to start an http server to make requests to.
        let http_router = axum::Router::new().route(
            FAIL_STATUS_PATH,
            axum::routing::get(|| async { axum::http::StatusCode::BAD_GATEWAY }),
        );
        let http_listener = tokio::net::TcpListener::bind(HTTP_SERVER_ADDR)
            .await
            .unwrap();
        let _http_server_handle = tokio_util::task::AbortOnDropHandle::new(tokio::task::spawn(
            axum::serve(http_listener, http_router).into_future(),
        ));

        // Create a reqwest error by making a request with a non-200 response.
        let reqwest_error = reqwest::get(format!("{fail_status_url}?api_key={SENSITIVE_VALUE}"))
            .await
            .unwrap()
            .error_for_status()
            .unwrap_err();

        // Create a Solana error from the reqwest error.
        let solana_error = client_error::Error {
            request: None,
            kind: client_error::ErrorKind::Reqwest(reqwest_error),
        };

        // Ensure that the original error contains the URL including the sensitive value.
        assert!(solana_error.to_string().contains(&fail_status_url));
        assert!(solana_error.to_string().contains(SENSITIVE_VALUE));

        // Ensure that the redacted error contains the URL _without_ the sensitive value.
        let redacted_error = redact_solana_error(solana_error);
        assert!(redacted_error.to_string().contains(&fail_status_url));
        assert!(!redacted_error.to_string().contains(SENSITIVE_VALUE));
    }
}
