use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;

use crate::{
    social::{SocialProvider, OAuthClient, OAuthToken, RateLimiter, ProviderConfig},
    SocialIdentity, SocialPlatform, Result,
    errors::ZkpAuthError,
};
use std::collections::HashMap;

/// Twitter API user response
#[derive(Debug, Deserialize)]
struct TwitterUser {
    data: TwitterUserData,
}

#[derive(Debug, Deserialize)]
struct TwitterUserData {
    id: String,
    username: String,
    name: String,
    #[serde(default)]
    verified: bool,
    #[serde(default)]
    public_metrics: Option<TwitterMetrics>,
    created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TwitterMetrics {
    followers_count: u64,
    following_count: u64,
    tweet_count: u64,
    #[allow(dead_code)]
    listed_count: u64,
}

/// Twitter/X OAuth provider
pub struct TwitterProvider {
    oauth_client: OAuthClient,
    api_base_url: String,
}

impl TwitterProvider {
    pub fn new(config: ProviderConfig) -> Result<Self> {
        let rate_limiter = if let (Some(requests), Some(window)) =
            (config.rate_limit_requests, config.rate_limit_window_secs) {
            Some(RateLimiter::new(requests, Duration::from_secs(window)))
        } else {
            None
        };

        let oauth_client = OAuthClient::new(config.oauth.clone(), rate_limiter)?;
        let api_base_url = config.api_base_url
            .unwrap_or_else(|| "https://api.twitter.com/2".to_string());

        Ok(Self {
            oauth_client,
            api_base_url,
        })
    }

    /// Get user information from Twitter API
    async fn get_user_info(&self, token: &OAuthToken) -> Result<TwitterUser> {
        let url = format!(
            "{}/users/me?user.fields=id,username,name,verified,public_metrics,created_at",
            self.api_base_url
        );

        self.oauth_client.api_request::<TwitterUser>(token, &url).await
    }
}

#[async_trait]
impl SocialProvider for TwitterProvider {
    async fn validate_token(&self, token: &str) -> Result<SocialIdentity> {
        // Parse the token as OAuthToken
        let oauth_token = serde_json::from_str::<OAuthToken>(token)
            .map_err(|e| ZkpAuthError::OAuthValidationFailed {
                reason: format!("Invalid token format: {}", e),
            })?;

        // Get user info from Twitter API
        let user_info = self.get_user_info(&oauth_token).await?;
        let user_data = user_info.data;

        // Calculate account age in days
        let account_age_days = user_data.created_at.as_ref().and_then(|created| {
            // Parse ISO 8601 timestamp
            chrono::DateTime::parse_from_rfc3339(created).ok()
                .map(|dt| {
                    let now = chrono::Utc::now();
                    (now - dt.with_timezone(&chrono::Utc)).num_days() as u64
                })
        });

        // Extract follower count
        let follower_count = user_data.public_metrics
            .as_ref()
            .map(|metrics| metrics.followers_count);

        let mut attributes = HashMap::new();
        attributes.insert("display_name".to_string(), user_data.name);
        if let Some(fc) = follower_count {
            attributes.insert("follower_count".to_string(), fc.to_string());
        }
        if let Some(age) = account_age_days {
            attributes.insert("account_age_days".to_string(), age.to_string());
        }
        if let Some(metrics) = user_data.public_metrics {
            attributes.insert("following_count".to_string(), metrics.following_count.to_string());
            attributes.insert("tweet_count".to_string(), metrics.tweet_count.to_string());
        }

        Ok(SocialIdentity {
            platform: SocialPlatform::Twitter,
            user_id: user_data.id,
            username: Some(user_data.username),
            email: None, // Twitter API doesn't provide email in v2 by default
            verified: user_data.verified,
            attributes,
        })
    }

    fn get_platform(&self) -> SocialPlatform {
        SocialPlatform::Twitter
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