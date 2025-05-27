use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

use crate::{
    client::HttpClient,
    error::Result,
    types::{
        AuthSession, EventListener, KolmeZkpConfig, KolmeZkpEvent, SocialPlatform,
        InitAuthRequest, InitAuthResponse, CallbackRequest, CallbackResponse,
        ProveIdentityRequest, ProveIdentityResponse, VerifyProofRequest, VerifyProofResponse,
        StatusQuery, StatusResponse, PublicKey, Signature,
    },
};

/// Main SDK client for Kolme ZKP authentication
pub struct KolmeZkpSdk {
    http_client: HttpClient,
    sessions: Arc<RwLock<HashMap<String, AuthSession>>>,
    event_listeners: Arc<RwLock<Vec<Arc<dyn EventListener>>>>,
    config: KolmeZkpConfig,
}

impl KolmeZkpSdk {
    /// Create a new SDK instance with the given configuration
    pub fn new(config: KolmeZkpConfig) -> Result<Self> {
        let http_client = HttpClient::new(&config)?;

        Ok(Self {
            http_client,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            event_listeners: Arc::new(RwLock::new(Vec::new())),
            config,
        })
    }

    /// Create a new SDK instance with default configuration
    pub fn with_api_url(api_url: impl Into<String>) -> Result<Self> {
        let config = KolmeZkpConfig::new(api_url);
        Self::new(config)
    }

    /// Add an event listener
    pub async fn add_event_listener(&self, listener: Arc<dyn EventListener>) {
        let mut listeners = self.event_listeners.write().await;
        listeners.push(listener);
    }

    /// Remove all event listeners
    pub async fn clear_event_listeners(&self) {
        let mut listeners = self.event_listeners.write().await;
        listeners.clear();
    }

    /// Emit an event to all listeners
    async fn emit_event(&self, event: KolmeZkpEvent) {
        let listeners = self.event_listeners.read().await;
        for listener in listeners.iter() {
            listener.on_event(event.clone()).await;
        }
    }

    /// Initialize OAuth authentication flow
    pub async fn init_auth(
        &self,
        platform: SocialPlatform,
        redirect_uri: impl Into<String>,
        public_key: Option<PublicKey>,
    ) -> Result<InitAuthResponse> {
        let request = InitAuthRequest {
            platform,
            redirect_uri: redirect_uri.into(),
            public_key,
        };

        log::info!("Initializing OAuth flow for platform: {:?}", platform);
        self.emit_event(KolmeZkpEvent::AuthStarted { platform }).await;

        match self.http_client.post::<InitAuthRequest, InitAuthResponse>("/auth/social/init", &request).await {
            Ok(response) => {
                log::info!("OAuth initialization successful");
                Ok(response)
            }
            Err(e) => {
                log::error!("OAuth initialization failed: {}", e);
                self.emit_event(KolmeZkpEvent::AuthFailed {
                    error: e.to_string(),
                }).await;
                Err(e)
            }
        }
    }

    /// Handle OAuth callback
    pub async fn handle_callback(
        &self,
        code: impl Into<String>,
        state: impl Into<String>,
    ) -> Result<CallbackResponse> {
        let request = CallbackRequest {
            code: code.into(),
            state: state.into(),
        };

        log::info!("Handling OAuth callback");

        match self.http_client.post::<CallbackRequest, CallbackResponse>("/auth/social/callback", &request).await {
            Ok(response) => {
                if response.success {
                    if let (Some(identity), Some(session_id)) = (&response.identity, &response.session_id) {
                        // Store session locally
                        let session = AuthSession::new(session_id.clone(), identity.clone());
                        let mut sessions = self.sessions.write().await;
                        sessions.insert(session_id.clone(), session);

                        log::info!("Authentication successful for user: {}", identity.user_id);
                        self.emit_event(KolmeZkpEvent::AuthCompleted {
                            identity: identity.clone(),
                            session_id: Some(session_id.clone()),
                        }).await;
                    }
                } else {
                    log::warn!("Authentication failed");
                    self.emit_event(KolmeZkpEvent::AuthFailed {
                        error: "Authentication failed".to_string(),
                    }).await;
                }
                Ok(response)
            }
            Err(e) => {
                log::error!("OAuth callback failed: {}", e);
                self.emit_event(KolmeZkpEvent::AuthFailed {
                    error: e.to_string(),
                }).await;
                Err(e)
            }
        }
    }

    /// Generate a zero-knowledge proof for the authenticated identity
    pub async fn prove_identity(
        &self,
        session_id: impl Into<String>,
        signature: Signature,
    ) -> Result<ProveIdentityResponse> {
        let session_id = session_id.into();
        let request = ProveIdentityRequest {
            session_id: session_id.clone(),
            signature,
        };

        log::info!("Generating ZKP proof for session: {}", session_id);

        // Update local session access time
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(&session_id) {
                session.touch();
            }
        }

        match self.http_client.post::<ProveIdentityRequest, ProveIdentityResponse>("/auth/social/prove", &request).await {
            Ok(response) => {
                log::info!("ZKP proof generated successfully");

                // Update local session with commitment
                {
                    let mut sessions = self.sessions.write().await;
                    if let Some(session) = sessions.get_mut(&session_id) {
                        session.commitment = Some(response.commitment.clone());
                    }
                }

                self.emit_event(KolmeZkpEvent::ProofGenerated {
                    commitment: response.commitment.clone(),
                    proof: response.proof.clone(),
                }).await;

                Ok(response)
            }
            Err(e) => {
                log::error!("ZKP proof generation failed: {}", e);

                if e.is_session_expired() {
                    self.emit_event(KolmeZkpEvent::SessionExpired {
                        session_id: session_id.clone(),
                    }).await;

                    // Remove expired session
                    let mut sessions = self.sessions.write().await;
                    sessions.remove(&session_id);
                }

                Err(e)
            }
        }
    }

    /// Verify a zero-knowledge proof
    pub async fn verify_proof(&self, request: VerifyProofRequest) -> Result<VerifyProofResponse> {
        log::info!("Verifying ZKP proof for platform: {:?}", request.platform);

        match self.http_client.post::<VerifyProofRequest, VerifyProofResponse>("/auth/social/verify", &request).await {
            Ok(response) => {
                log::info!("ZKP proof verification completed: valid={}", response.valid);

                self.emit_event(KolmeZkpEvent::ProofVerified {
                    valid: response.valid,
                    platform: response.platform,
                }).await;

                Ok(response)
            }
            Err(e) => {
                log::error!("ZKP proof verification failed: {}", e);
                Err(e)
            }
        }
    }

    /// Get the status of an authentication session
    pub async fn get_session_status(&self, session_id: impl Into<String>) -> Result<StatusResponse> {
        let session_id = session_id.into();
        let query = StatusQuery {
            session_id: session_id.clone(),
        };

        log::debug!("Getting session status for: {}", session_id);

        match self.http_client.get_with_query::<StatusQuery, StatusResponse>("/auth/social/status", &query).await {
            Ok(response) => {
                if !response.authenticated {
                    log::info!("Session expired: {}", session_id);
                    self.emit_event(KolmeZkpEvent::SessionExpired {
                        session_id: session_id.clone(),
                    }).await;

                    // Remove expired session
                    let mut sessions = self.sessions.write().await;
                    sessions.remove(&session_id);
                } else {
                    // Update local session
                    let mut sessions = self.sessions.write().await;
                    if let Some(session) = sessions.get_mut(&session_id) {
                        session.touch();
                        if let Some(commitment) = &response.commitment {
                            session.commitment = Some(commitment.clone());
                        }
                    }
                }

                Ok(response)
            }
            Err(e) => {
                log::error!("Failed to get session status: {}", e);
                Err(e)
            }
        }
    }

    /// Get a local session by ID
    pub async fn get_local_session(&self, session_id: &str) -> Option<AuthSession> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// List all local sessions
    pub async fn list_local_sessions(&self) -> Vec<AuthSession> {
        let sessions = self.sessions.read().await;
        sessions.values().cloned().collect()
    }

    /// Remove a local session
    pub async fn remove_local_session(&self, session_id: &str) -> bool {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id).is_some()
    }

    /// Clean up expired local sessions
    pub async fn cleanup_expired_sessions(&self) -> usize {
        let mut sessions = self.sessions.write().await;
        let initial_count = sessions.len();

        sessions.retain(|_, session| !session.is_expired());

        let removed_count = initial_count - sessions.len();
        if removed_count > 0 {
            log::info!("Cleaned up {} expired sessions", removed_count);
        }

        removed_count
    }

    /// Get the SDK configuration
    pub fn config(&self) -> &KolmeZkpConfig {
        &self.config
    }
}
