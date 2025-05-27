use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;

use crate::{
    social::{SocialProvider, OAuthClient, OAuthToken, RateLimiter, ProviderConfig},
    SocialIdentity, SocialPlatform, Result,
    errors::ZkpAuthError,
};
use std::collections::HashMap;

/// GitHub API user response
#[derive(Debug, Deserialize)]
struct GitHubUser {
    id: u64,
    login: String,
    name: Option<String>,
    email: Option<String>,
    #[serde(default)]
    followers: u64,
    #[serde(default)]
    following: u64,
    #[serde(default)]
    public_repos: u64,
    #[serde(default)]
    #[allow(dead_code)]
    public_gists: u64,
    created_at: String,
    #[serde(default)]
    two_factor_authentication: Option<bool>,
    #[serde(rename = "type")]
    user_type: Option<String>,
}

/// GitHub OAuth provider
pub struct GitHubProvider {
    oauth_client: OAuthClient,
    api_base_url: String,
}

impl GitHubProvider {
    pub fn new(config: ProviderConfig) -> Result<Self> {
        let rate_limiter = if let (Some(requests), Some(window)) =
            (config.rate_limit_requests, config.rate_limit_window_secs) {
            Some(RateLimiter::new(requests, Duration::from_secs(window)))
        } else {
            None
        };

        let oauth_client = OAuthClient::new(config.oauth.clone(), rate_limiter)?;
        let api_base_url = config.api_base_url
            .unwrap_or_else(|| "https://api.github.com".to_string());

        Ok(Self {
            oauth_client,
            api_base_url,
        })
    }

    /// Get user information from GitHub API
    async fn get_user_info(&self, token: &OAuthToken) -> Result<GitHubUser> {
        let url = format!("{}/user", self.api_base_url);
        self.oauth_client.api_request::<GitHubUser>(token, &url).await
    }
}

#[async_trait]
impl SocialProvider for GitHubProvider {
    async fn validate_token(&self, token: &str) -> Result<SocialIdentity> {
        // Parse the token as OAuthToken
        let oauth_token = serde_json::from_str::<OAuthToken>(token)
            .map_err(|e| ZkpAuthError::OAuthValidationFailed {
                reason: format!("Invalid token format: {}", e),
            })?;

        // Get user info from GitHub API
        let user_info = self.get_user_info(&oauth_token).await?;

        // Calculate account age in days
        let account_age_days = chrono::DateTime::parse_from_rfc3339(&user_info.created_at).ok()
            .map(|dt| {
                let now = chrono::Utc::now();
                (now - dt.with_timezone(&chrono::Utc)).num_days() as u64
            });

        // GitHub doesn't have a "verified" badge like Twitter, but we can check if they're a GitHub employee
        let verified = user_info.user_type.as_deref() == Some("User") &&
                      user_info.two_factor_authentication.unwrap_or(false);

        let mut attributes = HashMap::new();
        if let Some(name) = user_info.name {
            attributes.insert("display_name".to_string(), name);
        }
        attributes.insert("follower_count".to_string(), user_info.followers.to_string());
        attributes.insert("public_repos".to_string(), user_info.public_repos.to_string());
        attributes.insert("following_count".to_string(), user_info.following.to_string());
        if let Some(age) = account_age_days {
            attributes.insert("account_age_days".to_string(), age.to_string());
        }

        Ok(SocialIdentity {
            platform: SocialPlatform::GitHub,
            user_id: user_info.id.to_string(),
            username: Some(user_info.login),
            email: user_info.email,
            verified: verified,
            attributes,
        })
    }

    fn get_platform(&self) -> SocialPlatform {
        SocialPlatform::GitHub
    }

    fn get_auth_url(&self, state: &str, redirect_uri: &str) -> String {
        self.oauth_client.get_auth_url(state, redirect_uri)
    }

    async fn exchange_code(&self, code: &str, _state: &str) -> Result<String> {
        let token = self.oauth_client.exchange_code(code).await?;

        // Serialize token to JSON string
        serde_json::to_string(&token)
            .map_err(|e| ZkpAuthError::SerializationError {
                reason: e.to_string(),
            })
    }
}