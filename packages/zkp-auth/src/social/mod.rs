use async_trait::async_trait;
use std::collections::HashMap;

use crate::{SocialIdentity, SocialPlatform, Result};

pub mod config;
pub mod oauth_client;
pub mod twitter;
pub mod github;
pub mod discord;
pub mod google;

pub use config::{ConfigManager, OAuthConfig, ProviderConfig};
pub use oauth_client::{OAuthClient, OAuthToken, RateLimiter};

/// Trait for social identity providers
#[async_trait]
pub trait SocialProvider: Send + Sync {
    /// Validate an OAuth token and return the social identity
    async fn validate_token(&self, token: &str) -> Result<SocialIdentity>;
    
    /// Get the platform this provider handles
    fn get_platform(&self) -> SocialPlatform;
    
    /// Get the OAuth authorization URL
    fn get_auth_url(&self, state: &str, redirect_uri: &str) -> String;
    
    /// Exchange authorization code for access token
    async fn exchange_code(&self, code: &str, state: &str) -> Result<String>;
}

/// Registry of social providers
pub struct ProviderRegistry {
    providers: HashMap<SocialPlatform, Box<dyn SocialProvider>>,
}

impl ProviderRegistry {
    /// Create a new provider registry
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }
    
    /// Create a provider registry from configuration
    pub fn from_config(config_manager: &ConfigManager) -> Result<Self> {
        let mut registry = Self::new();
        
        // Try to register Twitter provider
        if let Ok(config) = config_manager.get_provider(SocialPlatform::Twitter) {
            let provider = twitter::TwitterProvider::new(config.clone())?;
            registry.register(Box::new(provider));
        }
        
        // Try to register GitHub provider
        if let Ok(config) = config_manager.get_provider(SocialPlatform::GitHub) {
            let provider = github::GitHubProvider::new(config.clone())?;
            registry.register(Box::new(provider));
        }
        
        // Try to register Discord provider
        if let Ok(config) = config_manager.get_provider(SocialPlatform::Discord) {
            let provider = discord::DiscordProvider::new(config.clone())?;
            registry.register(Box::new(provider));
        }
        
        // Try to register Google provider
        if let Ok(config) = config_manager.get_provider(SocialPlatform::Google) {
            let provider = google::GoogleProvider::new(config.clone())?;
            registry.register(Box::new(provider));
        }
        
        Ok(registry)
    }
    
    /// Register a social provider
    pub fn register(&mut self, provider: Box<dyn SocialProvider>) {
        let platform = provider.get_platform();
        self.providers.insert(platform, provider);
    }
    
    /// Get a provider by platform
    pub fn get(&self, platform: SocialPlatform) -> Option<&dyn SocialProvider> {
        self.providers.get(&platform).map(|p| p.as_ref())
    }
    
    /// Get a mutable provider by platform
    pub fn get_mut(&mut self, platform: SocialPlatform) -> Option<&mut (dyn SocialProvider + '_)> {
        if let Some(provider) = self.providers.get_mut(&platform) {
            Some(provider.as_mut())
        } else {
            None
        }
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}