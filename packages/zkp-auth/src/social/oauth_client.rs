use oauth2::{
    AuthorizationCode, AuthUrl, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use oauth2::basic::{BasicClient, BasicTokenResponse};
use oauth2::reqwest::async_http_client;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::{Result, errors::ZkpAuthError};
use super::config::OAuthConfig;

/// OAuth token with expiration tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub token_type: String,
    pub expires_in_secs: Option<u64>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
}

impl From<BasicTokenResponse> for OAuthToken {
    fn from(response: BasicTokenResponse) -> Self {
        let expires_in_secs = response.expires_in().map(|duration| {
            duration.as_secs()
        });
        
        Self {
            access_token: response.access_token().secret().clone(),
            token_type: match response.token_type() {
                oauth2::basic::BasicTokenType::Bearer => "Bearer".to_string(),
                oauth2::basic::BasicTokenType::Mac => "Mac".to_string(),
                oauth2::basic::BasicTokenType::Extension(s) => s.clone(),
            },
            expires_in_secs,
            refresh_token: response.refresh_token().map(|t| t.secret().clone()),
            scope: response.scopes().map(|scopes| {
                scopes.iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            }),
        }
    }
}

/// Rate limiter for API calls
#[derive(Debug)]
pub struct RateLimiter {
    requests: Mutex<Vec<Instant>>,
    max_requests: u32,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window: Duration) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            max_requests,
            window,
        }
    }
    
    pub async fn check_rate_limit(&self) -> Result<()> {
        let mut requests = self.requests.lock().await;
        let now = Instant::now();
        
        // Remove old requests outside the window
        requests.retain(|&req| now.duration_since(req) < self.window);
        
        if requests.len() >= self.max_requests as usize {
            let oldest = requests.first().copied().unwrap_or(now);
            let wait_time = self.window - now.duration_since(oldest);
            return Err(ZkpAuthError::OAuthValidationFailed {
                reason: format!("Rate limit exceeded. Try again in {:?}", wait_time),
            });
        }
        
        requests.push(now);
        Ok(())
    }
}

/// Base OAuth client for social providers
pub struct OAuthClient {
    client: BasicClient,
    config: OAuthConfig,
    rate_limiter: Option<RateLimiter>,
}

impl OAuthClient {
    /// Create a new OAuth client
    pub fn new(config: OAuthConfig, rate_limiter: Option<RateLimiter>) -> Result<Self> {
        let client = BasicClient::new(
            ClientId::new(config.client_id.clone()),
            Some(ClientSecret::new(config.client_secret.clone())),
            AuthUrl::new(config.auth_url.clone())
                .map_err(|e| ZkpAuthError::OAuthValidationFailed {
                    reason: format!("Invalid auth URL: {}", e),
                })?,
            Some(TokenUrl::new(config.token_url.clone())
                .map_err(|e| ZkpAuthError::OAuthValidationFailed {
                    reason: format!("Invalid token URL: {}", e),
                })?),
        );
        
        Ok(Self {
            client,
            config: config.clone(),
            rate_limiter,
        })
    }
    
    /// Generate authorization URL without PKCE (for simpler flow)
    pub fn get_auth_url(&self, state: &str, redirect_uri: &str) -> String {
        let mut auth_request = self.client
            .authorize_url(|| CsrfToken::new(state.to_string()))
            .set_redirect_uri(std::borrow::Cow::Owned(RedirectUrl::new(redirect_uri.to_string()).unwrap()));
        
        // Add scopes
        for scope in &self.config.scopes {
            auth_request = auth_request.add_scope(Scope::new(scope.clone()));
        }
        
        // Add additional parameters
        for (key, value) in &self.config.additional_params {
            auth_request = auth_request.add_extra_param(key, value);
        }
        
        let (url, _csrf_token) = auth_request.url();
        url.to_string()
    }
    
    /// Exchange authorization code for access token (simplified without PKCE)
    pub async fn exchange_code(&self, code: &str) -> Result<String> {
        // Check rate limit if configured
        if let Some(limiter) = &self.rate_limiter {
            limiter.check_rate_limit().await?;
        }
        
        let token_response = self.client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .request_async(async_http_client)
            .await
            .map_err(|e| ZkpAuthError::OAuthValidationFailed {
                reason: format!("Token exchange failed: {}", e),
            })?;
        
        Ok(token_response.access_token().secret().clone())
    }
    
    /// Generate authorization URL with PKCE
    pub fn get_authorization_url_with_pkce(&self, redirect_uri: &str, state: &str) -> Result<(String, String)> {
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        
        let mut auth_request = self.client
            .authorize_url(|| CsrfToken::new(state.to_string()))
            .set_redirect_uri(std::borrow::Cow::Owned(
                RedirectUrl::new(redirect_uri.to_string())
                    .map_err(|e| ZkpAuthError::OAuthValidationFailed {
                        reason: format!("Invalid redirect URI: {}", e),
                    })?
            ))
            .set_pkce_challenge(pkce_challenge);
        
        // Add scopes
        for scope in &self.config.scopes {
            auth_request = auth_request.add_scope(Scope::new(scope.clone()));
        }
        
        // Add additional parameters
        for (key, value) in &self.config.additional_params {
            auth_request = auth_request.add_extra_param(key, value);
        }
        
        let (url, _csrf_token) = auth_request.url();
        
        Ok((url.to_string(), pkce_verifier.secret().clone()))
    }
    
    /// Exchange authorization code for access token with PKCE
    pub async fn exchange_code_with_pkce(&self, code: &str, redirect_uri: &str, pkce_verifier: &str) -> Result<OAuthToken> {
        // Check rate limit if configured
        if let Some(limiter) = &self.rate_limiter {
            limiter.check_rate_limit().await?;
        }
        
        let token_response = self.client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .set_redirect_uri(std::borrow::Cow::Owned(
                RedirectUrl::new(redirect_uri.to_string())
                    .map_err(|e| ZkpAuthError::OAuthValidationFailed {
                        reason: format!("Invalid redirect URI: {}", e),
                    })?
            ))
            .set_pkce_verifier(oauth2::PkceCodeVerifier::new(pkce_verifier.to_string()))
            .request_async(async_http_client)
            .await
            .map_err(|e| ZkpAuthError::OAuthValidationFailed {
                reason: format!("Token exchange failed: {}", e),
            })?;
        
        Ok(OAuthToken::from(token_response))
    }
    
    /// Make an authenticated API request
    pub async fn api_request<T: for<'de> Deserialize<'de>>(
        &self,
        token: &OAuthToken,
        url: &str,
    ) -> Result<T> {
        // Check rate limit if configured
        if let Some(limiter) = &self.rate_limiter {
            limiter.check_rate_limit().await?;
        }
        
        let client = reqwest::Client::new();
        let response = client
            .get(url)
            .header("Authorization", format!("{} {}", token.token_type, token.access_token))
            .header("User-Agent", "Kolme-ZKP-Auth/1.0")
            .send()
            .await
            .map_err(|e| ZkpAuthError::SocialProviderError {
                provider: "oauth".to_string(),
                reason: format!("API request failed: {}", e),
            })?;
        
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ZkpAuthError::SocialProviderError {
                provider: "oauth".to_string(),
                reason: format!("API request failed with status {}: {}", status, body),
            });
        }
        
        response.json::<T>().await
            .map_err(|e| ZkpAuthError::SocialProviderError {
                provider: "oauth".to_string(),
                reason: format!("Failed to parse response: {}", e),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_rate_limiter() {
        let limiter = RateLimiter::new(2, Duration::from_secs(1));
        
        // First two requests should succeed
        assert!(limiter.check_rate_limit().await.is_ok());
        assert!(limiter.check_rate_limit().await.is_ok());
        
        // Third request should fail
        assert!(limiter.check_rate_limit().await.is_err());
        
        // Wait for window to pass
        tokio::time::sleep(Duration::from_secs(1)).await;
        
        // Should succeed again
        assert!(limiter.check_rate_limit().await.is_ok());
    }
}