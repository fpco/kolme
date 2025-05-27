use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{SocialPlatform, Result, errors::ZkpAuthError};

/// OAuth provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub auth_url: String,
    pub token_url: String,
    pub user_info_url: String,
    pub scopes: Vec<String>,
    #[serde(default)]
    pub additional_params: HashMap<String, String>,
}

/// Social provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub platform: SocialPlatform,
    pub oauth: OAuthConfig,
    #[serde(default)]
    pub api_base_url: Option<String>,
    #[serde(default)]
    pub rate_limit_requests: Option<u32>,
    #[serde(default)]
    pub rate_limit_window_secs: Option<u64>,
}

impl ProviderConfig {
    /// Create Twitter/X provider configuration
    pub fn twitter(client_id: String, client_secret: String) -> Self {
        Self {
            platform: SocialPlatform::Twitter,
            oauth: OAuthConfig {
                client_id,
                client_secret,
                auth_url: "https://twitter.com/i/oauth2/authorize".to_string(),
                token_url: "https://api.twitter.com/2/oauth2/token".to_string(),
                user_info_url: "https://api.twitter.com/2/users/me".to_string(),
                scopes: vec!["tweet.read".to_string(), "users.read".to_string()],
                additional_params: {
                    let mut params = HashMap::new();
                    params.insert("code_challenge_method".to_string(), "S256".to_string());
                    params
                },
            },
            api_base_url: Some("https://api.twitter.com/2".to_string()),
            rate_limit_requests: Some(300),
            rate_limit_window_secs: Some(900), // 15 minutes
        }
    }
    
    /// Create GitHub provider configuration
    pub fn github(client_id: String, client_secret: String) -> Self {
        Self {
            platform: SocialPlatform::GitHub,
            oauth: OAuthConfig {
                client_id,
                client_secret,
                auth_url: "https://github.com/login/oauth/authorize".to_string(),
                token_url: "https://github.com/login/oauth/access_token".to_string(),
                user_info_url: "https://api.github.com/user".to_string(),
                scopes: vec!["read:user".to_string()],
                additional_params: HashMap::new(),
            },
            api_base_url: Some("https://api.github.com".to_string()),
            rate_limit_requests: Some(5000),
            rate_limit_window_secs: Some(3600), // 1 hour
        }
    }
    
    /// Create Discord provider configuration
    pub fn discord(client_id: String, client_secret: String) -> Self {
        Self {
            platform: SocialPlatform::Discord,
            oauth: OAuthConfig {
                client_id,
                client_secret,
                auth_url: "https://discord.com/api/oauth2/authorize".to_string(),
                token_url: "https://discord.com/api/oauth2/token".to_string(),
                user_info_url: "https://discord.com/api/users/@me".to_string(),
                scopes: vec!["identify".to_string()],
                additional_params: HashMap::new(),
            },
            api_base_url: Some("https://discord.com/api/v10".to_string()),
            rate_limit_requests: Some(50),
            rate_limit_window_secs: Some(1), // Per second
        }
    }
    
    /// Create Google provider configuration
    pub fn google(client_id: String, client_secret: String) -> Self {
        Self {
            platform: SocialPlatform::Google,
            oauth: OAuthConfig {
                client_id,
                client_secret,
                auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
                token_url: "https://oauth2.googleapis.com/token".to_string(),
                user_info_url: "https://www.googleapis.com/oauth2/v2/userinfo".to_string(),
                scopes: vec![
                    "openid".to_string(),
                    "email".to_string(),
                    "profile".to_string(),
                ],
                additional_params: HashMap::new(),
            },
            api_base_url: Some("https://www.googleapis.com".to_string()),
            rate_limit_requests: Some(1000),
            rate_limit_window_secs: Some(100), // Per 100 seconds
        }
    }
}

/// Configuration manager for all social providers
#[derive(Debug, Clone, Default)]
pub struct ConfigManager {
    configs: HashMap<SocialPlatform, ProviderConfig>,
}

impl ConfigManager {
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
        }
    }
    
    /// Add a provider configuration
    pub fn add_provider(&mut self, config: ProviderConfig) {
        self.configs.insert(config.platform, config);
    }
    
    /// Get a provider configuration
    pub fn get_provider(&self, platform: SocialPlatform) -> Result<&ProviderConfig> {
        self.configs
            .get(&platform)
            .ok_or_else(|| ZkpAuthError::InvalidPlatform {
                platform: platform.as_str().to_string(),
            })
    }
    
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        let mut manager = Self::new();
        
        // Try to load Twitter config
        if let (Ok(id), Ok(secret)) = (
            std::env::var("TWITTER_CLIENT_ID"),
            std::env::var("TWITTER_CLIENT_SECRET"),
        ) {
            manager.add_provider(ProviderConfig::twitter(id, secret));
        }
        
        // Try to load GitHub config
        if let (Ok(id), Ok(secret)) = (
            std::env::var("GITHUB_CLIENT_ID"),
            std::env::var("GITHUB_CLIENT_SECRET"),
        ) {
            manager.add_provider(ProviderConfig::github(id, secret));
        }
        
        // Try to load Discord config
        if let (Ok(id), Ok(secret)) = (
            std::env::var("DISCORD_CLIENT_ID"),
            std::env::var("DISCORD_CLIENT_SECRET"),
        ) {
            manager.add_provider(ProviderConfig::discord(id, secret));
        }
        
        // Try to load Google config
        if let (Ok(id), Ok(secret)) = (
            std::env::var("GOOGLE_CLIENT_ID"),
            std::env::var("GOOGLE_CLIENT_SECRET"),
        ) {
            manager.add_provider(ProviderConfig::google(id, secret));
        }
        
        Ok(manager)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_provider_configs() {
        let twitter = ProviderConfig::twitter("id".to_string(), "secret".to_string());
        assert_eq!(twitter.platform, SocialPlatform::Twitter);
        assert!(twitter.oauth.scopes.contains(&"tweet.read".to_string()));
        
        let github = ProviderConfig::github("id".to_string(), "secret".to_string());
        assert_eq!(github.platform, SocialPlatform::GitHub);
        assert!(github.oauth.scopes.contains(&"read:user".to_string()));
    }
    
    #[test]
    fn test_config_manager() {
        let mut manager = ConfigManager::new();
        let config = ProviderConfig::twitter("id".to_string(), "secret".to_string());
        manager.add_provider(config);
        
        assert!(manager.get_provider(SocialPlatform::Twitter).is_ok());
        assert!(manager.get_provider(SocialPlatform::GitHub).is_err());
    }
}