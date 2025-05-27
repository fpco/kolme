use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;

use crate::{
    social::{SocialProvider, OAuthClient, OAuthToken, RateLimiter, ProviderConfig},
    SocialIdentity, SocialPlatform, Result,
    errors::ZkpAuthError,
};
use std::collections::HashMap;

/// Discord API user response
#[derive(Debug, Deserialize)]
struct DiscordUser {
    id: String,
    username: String,
    email: Option<String>,
    verified: Option<bool>,
    discriminator: Option<String>,
    avatar: Option<String>,
    #[serde(default)]
    premium_type: Option<u8>,
}

/// Discord OAuth provider
pub struct DiscordProvider {
    oauth_client: OAuthClient,
    api_base_url: String,
}

impl DiscordProvider {
    pub fn new(config: ProviderConfig) -> Result<Self> {
        let rate_limiter = if let (Some(requests), Some(window)) = 
            (config.rate_limit_requests, config.rate_limit_window_secs) {
            Some(RateLimiter::new(requests, Duration::from_secs(window)))
        } else {
            None
        };
        
        let oauth_client = OAuthClient::new(config.oauth.clone(), rate_limiter)?;
        let api_base_url = config.api_base_url
            .unwrap_or_else(|| "https://discord.com/api/v10".to_string());
        
        Ok(Self {
            oauth_client,
            api_base_url,
        })
    }
    
    /// Get user information from Discord API
    async fn get_user_info(&self, token: &OAuthToken) -> Result<DiscordUser> {
        let url = format!("{}/users/@me", self.api_base_url);
        self.oauth_client.api_request::<DiscordUser>(token, &url).await
    }
}

#[async_trait]
impl SocialProvider for DiscordProvider {
    async fn validate_token(&self, token: &str) -> Result<SocialIdentity> {
        // Parse the token as OAuthToken
        let oauth_token = serde_json::from_str::<OAuthToken>(token)
            .map_err(|e| ZkpAuthError::OAuthValidationFailed {
                reason: format!("Invalid token format: {}", e),
            })?;
        
        // Get user info from Discord API
        let user_info = self.get_user_info(&oauth_token).await?;
        
        let mut attributes = HashMap::new();
        if let Some(discriminator) = user_info.discriminator {
            attributes.insert("discriminator".to_string(), discriminator);
        }
        if let Some(avatar) = user_info.avatar {
            attributes.insert("avatar".to_string(), avatar);
        }
        if let Some(premium) = user_info.premium_type {
            attributes.insert("premium_type".to_string(), premium.to_string());
        }

        Ok(SocialIdentity {
            platform: SocialPlatform::Discord,
            user_id: user_info.id,
            username: Some(user_info.username),
            email: user_info.email,
            verified: user_info.verified.unwrap_or(false),
            attributes,
        })
    }
    
    fn get_platform(&self) -> SocialPlatform {
        SocialPlatform::Discord
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