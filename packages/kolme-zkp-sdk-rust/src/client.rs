use reqwest::{Client, ClientBuilder, Response};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use url::Url;

use crate::{
    error::{KolmeZkpError, Result},
    types::{ApiError, KolmeZkpConfig},
};

/// HTTP client for making requests to the Kolme API
#[derive(Debug, Clone)]
pub struct HttpClient {
    client: Client,
    base_url: Url,
}

impl HttpClient {
    /// Create a new HTTP client with the given configuration
    pub fn new(config: &KolmeZkpConfig) -> Result<Self> {
        let mut builder = ClientBuilder::new();

        // Set timeout
        if let Some(timeout) = config.timeout {
            builder = builder.timeout(Duration::from_secs(timeout));
        }

        // Set SSL verification
        if !config.verify_ssl {
            builder = builder.danger_accept_invalid_certs(true);
        }

        // Add default headers
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static("kolme-zkp-sdk-rust/0.1.0"),
        );

        // Add custom headers
        for (key, value) in &config.headers {
            let header_name = reqwest::header::HeaderName::from_bytes(key.as_bytes())
                .map_err(|e| KolmeZkpError::config(format!("Invalid header name '{}': {}", key, e)))?;
            let header_value = reqwest::header::HeaderValue::from_str(value)
                .map_err(|e| KolmeZkpError::config(format!("Invalid header value '{}': {}", value, e)))?;
            headers.insert(header_name, header_value);
        }

        builder = builder.default_headers(headers);

        let client = builder.build()?;
        let base_url = Url::parse(&config.api_url)?;

        Ok(Self { client, base_url })
    }

    /// Make a GET request
    pub async fn get<T>(&self, path: &str) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let url = self.build_url(path)?;
        log::debug!("GET {}", url);

        let response = self.client.get(url).send().await?;
        self.handle_response(response).await
    }

    /// Make a GET request with query parameters
    pub async fn get_with_query<Q, T>(&self, path: &str, query: &Q) -> Result<T>
    where
        Q: Serialize,
        T: for<'de> Deserialize<'de>,
    {
        let url = self.build_url(path)?;
        log::debug!("GET {} with query", url);

        let response = self.client.get(url).query(query).send().await?;
        self.handle_response(response).await
    }

    /// Make a POST request with JSON body
    pub async fn post<B, T>(&self, path: &str, body: &B) -> Result<T>
    where
        B: Serialize,
        T: for<'de> Deserialize<'de>,
    {
        let url = self.build_url(path)?;
        log::debug!("POST {}", url);

        let response = self.client.post(url).json(body).send().await?;
        self.handle_response(response).await
    }

    /// Make a PUT request with JSON body
    pub async fn put<B, T>(&self, path: &str, body: &B) -> Result<T>
    where
        B: Serialize,
        T: for<'de> Deserialize<'de>,
    {
        let url = self.build_url(path)?;
        log::debug!("PUT {}", url);

        let response = self.client.put(url).json(body).send().await?;
        self.handle_response(response).await
    }

    /// Make a DELETE request
    pub async fn delete<T>(&self, path: &str) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let url = self.build_url(path)?;
        log::debug!("DELETE {}", url);

        let response = self.client.delete(url).send().await?;
        self.handle_response(response).await
    }

    /// Build a full URL from a path
    fn build_url(&self, path: &str) -> Result<Url> {
        let path = if path.starts_with('/') {
            &path[1..]
        } else {
            path
        };

        self.base_url
            .join(path)
            .map_err(|e| KolmeZkpError::config(format!("Invalid URL path '{}': {}", path, e)))
    }

    /// Handle HTTP response, converting errors to KolmeZkpError
    async fn handle_response<T>(&self, response: Response) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let status = response.status();
        let url = response.url().clone();

        log::debug!("Response: {} {}", status, url);

        if status.is_success() {
            let text = response.text().await?;
            log::trace!("Response body: {}", text);

            serde_json::from_str(&text).map_err(|e| {
                log::error!("Failed to parse response JSON: {}", e);
                log::error!("Response body: {}", text);
                KolmeZkpError::JsonError(e)
            })
        } else {
            let text = response.text().await?;
            log::warn!("HTTP error {}: {}", status, text);

            // Try to parse as API error
            if let Ok(api_error) = serde_json::from_str::<ApiError>(&text) {
                Err(KolmeZkpError::ApiError(api_error))
            } else {
                // Fallback to generic error
                let error_msg = if text.is_empty() {
                    format!("HTTP {} error", status)
                } else {
                    format!("HTTP {} error: {}", status, text)
                };
                Err(KolmeZkpError::custom(error_msg))
            }
        }
    }
}

/// Builder for creating HTTP clients with custom configuration
pub struct HttpClientBuilder {
    config: KolmeZkpConfig,
}

impl HttpClientBuilder {
    /// Create a new builder with the given base URL
    pub fn new(api_url: impl Into<String>) -> Self {
        Self {
            config: KolmeZkpConfig::new(api_url),
        }
    }

    /// Set the request timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = Some(timeout.as_secs());
        self
    }

    /// Add a custom header
    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.headers.insert(key.into(), value.into());
        self
    }

    /// Disable SSL certificate verification (for testing only)
    pub fn insecure(mut self) -> Self {
        self.config.verify_ssl = false;
        self
    }

    /// Build the HTTP client
    pub fn build(self) -> Result<HttpClient> {
        HttpClient::new(&self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn test_http_client_creation() {
        let config = KolmeZkpConfig::new("http://localhost:8080");
        let client = HttpClient::new(&config).unwrap();
        assert_eq!(client.base_url.as_str(), "http://localhost:8080/");
    }

    #[tokio::test]
    async fn test_url_building() {
        let config = KolmeZkpConfig::new("http://localhost:8080");
        let client = HttpClient::new(&config).unwrap();

        let url = client.build_url("/api/test").unwrap();
        assert_eq!(url.as_str(), "http://localhost:8080/api/test");

        let url = client.build_url("api/test").unwrap();
        assert_eq!(url.as_str(), "http://localhost:8080/api/test");
    }

    #[tokio::test]
    async fn test_successful_get_request() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/test")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"message": "success"}"#)
            .create_async()
            .await;

        let config = KolmeZkpConfig::new(server.url());
        let client = HttpClient::new(&config).unwrap();

        #[derive(Deserialize)]
        struct TestResponse {
            message: String,
        }

        let response: TestResponse = client.get("/test").await.unwrap();
        assert_eq!(response.message, "success");
    }

    #[tokio::test]
    async fn test_api_error_response() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/error")
            .with_status(400)
            .with_header("content-type", "application/json")
            .with_body(r#"{"error": "Bad request", "details": null}"#)
            .create_async()
            .await;

        let config = KolmeZkpConfig::new(server.url());
        let client = HttpClient::new(&config).unwrap();

        #[derive(Deserialize, Debug)]
        struct TestResponse {
            #[allow(dead_code)]
            message: String,
        }

        let result: Result<TestResponse> = client.get("/error").await;
        assert!(result.is_err());

        match result.unwrap_err() {
            KolmeZkpError::ApiError(api_error) => {
                assert_eq!(api_error.error, "Bad request");
            }
            _ => panic!("Expected ApiError"),
        }
    }

    #[tokio::test]
    async fn test_builder_pattern() {
        let client = HttpClientBuilder::new("http://localhost:8080")
            .timeout(Duration::from_secs(60))
            .header("X-Custom", "test")
            .build()
            .unwrap();

        assert_eq!(client.base_url.as_str(), "http://localhost:8080/");
    }
}
