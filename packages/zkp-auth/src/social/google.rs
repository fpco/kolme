use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;

use crate::{
    social::{SocialProvider, OAuthClient, OAuthToken, RateLimiter, ProviderConfig},
    SocialIdentity, SocialPlatform, Result,
    errors::ZkpAuthError,
};
use std::collections::HashMap;

/// Google API user response
#[derive(Debug, Deserialize)]
struct GoogleUser {
    id: String,
    email: String,
    verified_email: Option<bool>,
    name: Option<String>,
    given_name: Option<String>,
    family_name: Option<String>,
    picture: Option<String>,
    locale: Option<String>,
}

/// Google OAuth provider
pub struct GoogleProvider {
    oauth_client: OAuthClient,
    api_base_url: String,
}

impl GoogleProvider {
    pub fn new(config: ProviderConfig) -> Result<Self> {
        let rate_limiter = if let (Some(requests), Some(window)) = 
            (config.rate_limit_requests, config.rate_limit_window_secs) {
            Some(RateLimiter::new(requests, Duration::from_secs(window)))
        } else {
            None
        };
        
        let oauth_client = OAuthClient::new(config.oauth.clone(), rate_limiter)?;
        let api_base_url = config.api_base_url
            .unwrap_or_else(|| "https://www.googleapis.com".to_string());
        
        Ok(Self {
            oauth_client,
            api_base_url,
        })
    }
    
    /// Get user information from Google API
    async fn get_user_info(&self, token: &OAuthToken) -> Result<GoogleUser> {
        let url = format!("{}/oauth2/v2/userinfo", self.api_base_url);
        self.oauth_client.api_request::<GoogleUser>(token, &url).await
    }
}

#[async_trait]
impl SocialProvider for GoogleProvider {
    async fn validate_token(&self, token: &str) -> Result<SocialIdentity> {
        // Parse the token as OAuthToken
        let oauth_token = serde_json::from_str::<OAuthToken>(token)
            .map_err(|e| ZkpAuthError::OAuthValidationFailed {
                reason: format!("Invalid token format: {}", e),
            })?;
        
        // Get user info from Google API
        let user_info = self.get_user_info(&oauth_token).await?;
        
        let mut attributes = HashMap::new();
        if let Some(name) = user_info.name {
            attributes.insert("name".to_string(), name);
        }
        if let Some(given_name) = user_info.given_name {
            attributes.insert("given_name".to_string(), given_name);
        }
        if let Some(family_name) = user_info.family_name {
            attributes.insert("family_name".to_string(), family_name);
        }
        if let Some(picture) = user_info.picture {
            attributes.insert("picture".to_string(), picture);
        }
        if let Some(locale) = user_info.locale {
            attributes.insert("locale".to_string(), locale);
        }

        Ok(SocialIdentity {
            platform: SocialPlatform::Google,
            user_id: user_info.id,
            username: Some(user_info.email.clone()), // Google uses email as primary identifier
            email: Some(user_info.email),
            verified: user_info.verified_email.unwrap_or(false),
            attributes,
        })
    }
    
    fn get_platform(&self) -> SocialPlatform {
        SocialPlatform::Google
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