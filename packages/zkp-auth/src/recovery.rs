use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use crate::{SocialPlatform, SocialIdentityCommitment, ZkProof};
use anyhow::{Result, bail};

/// Configuration for social recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialRecoveryConfig {
    /// Minimum number of guardians required to initiate recovery
    pub threshold: u32,
    /// Time delay before recovery can be executed (in seconds)
    pub time_lock: u64,
    /// Maximum number of guardians allowed
    pub max_guardians: u32,
    /// Whether recovery is enabled
    pub enabled: bool,
}

impl Default for SocialRecoveryConfig {
    fn default() -> Self {
        Self {
            threshold: 2,
            time_lock: 86400, // 24 hours
            max_guardians: 5,
            enabled: true,
        }
    }
}

/// A recovery guardian linked to a social identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryGuardian {
    /// Social platform of the guardian
    pub platform: SocialPlatform,
    /// Identity commitment of the guardian
    pub commitment: SocialIdentityCommitment,
    /// When this guardian was added
    pub added_at: u64,
    /// Whether this guardian is active
    pub active: bool,
}

/// A recovery request initiated by guardians
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryRequest {
    /// Unique ID of the recovery request
    pub id: String,
    /// Account being recovered
    pub account_id: String,
    /// New public key to set for the account
    pub new_public_key: Vec<u8>,
    /// When the recovery was initiated
    pub initiated_at: u64,
    /// When the recovery can be executed
    pub executable_at: u64,
    /// Guardians who have approved this recovery
    pub approvals: BTreeSet<SocialIdentityCommitment>,
    /// Status of the recovery
    pub status: RecoveryStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RecoveryStatus {
    /// Waiting for more guardian approvals
    Pending,
    /// Has enough approvals, waiting for timelock
    TimeLocked,
    /// Ready to be executed
    Ready,
    /// Successfully executed
    Executed,
    /// Cancelled by account owner or timeout
    Cancelled,
}

/// Manager for social recovery operations
pub struct RecoveryManager {
    /// Active recovery requests by ID
    requests: BTreeMap<String, RecoveryRequest>,
    /// Recovery requests by account ID
    requests_by_account: BTreeMap<String, BTreeSet<String>>,
}

impl RecoveryManager {
    pub fn new() -> Self {
        Self {
            requests: BTreeMap::new(),
            requests_by_account: BTreeMap::new(),
        }
    }

    /// Initiate a recovery request
    pub fn initiate_recovery(
        &mut self,
        account_id: String,
        guardian_commitment: SocialIdentityCommitment,
        new_public_key: Vec<u8>,
        config: &SocialRecoveryConfig,
        current_time: u64,
    ) -> Result<String> {
        if !config.enabled {
            bail!("Social recovery is not enabled for this account");
        }

        // Generate unique recovery ID
        let recovery_id = format!("recovery_{}_{}_{}",
            account_id,
            current_time,
            hex::encode(&guardian_commitment.commitment[..8])
        );

        // Check if there's already an active recovery for this account
        if let Some(active_requests) = self.requests_by_account.get(&account_id) {
            for request_id in active_requests {
                if let Some(request) = self.requests.get(request_id) {
                    if matches!(request.status, RecoveryStatus::Pending | RecoveryStatus::TimeLocked | RecoveryStatus::Ready) {
                        bail!("There is already an active recovery request for this account");
                    }
                }
            }
        }

        // Create new recovery request
        let mut approvals = BTreeSet::new();
        approvals.insert(guardian_commitment);

        let request = RecoveryRequest {
            id: recovery_id.clone(),
            account_id: account_id.clone(),
            new_public_key,
            initiated_at: current_time,
            executable_at: current_time + config.time_lock,
            approvals,
            status: if config.threshold == 1 {
                RecoveryStatus::TimeLocked
            } else {
                RecoveryStatus::Pending
            },
        };

        // Store the request
        self.requests.insert(recovery_id.clone(), request);
        self.requests_by_account
            .entry(account_id)
            .or_insert_with(BTreeSet::new)
            .insert(recovery_id.clone());

        Ok(recovery_id)
    }

    /// Add guardian approval to a recovery request
    pub fn approve_recovery(
        &mut self,
        recovery_id: &str,
        guardian_commitment: SocialIdentityCommitment,
        config: &SocialRecoveryConfig,
        current_time: u64,
    ) -> Result<RecoveryStatus> {
        let request = self.requests.get_mut(recovery_id)
            .ok_or_else(|| anyhow::anyhow!("Recovery request not found"))?;

        // Check if recovery is still pending
        match request.status {
            RecoveryStatus::Executed => bail!("Recovery already executed"),
            RecoveryStatus::Cancelled => bail!("Recovery was cancelled"),
            _ => {}
        }

        // Add approval
        if !request.approvals.insert(guardian_commitment) {
            bail!("Guardian has already approved this recovery");
        }

        // Update status based on approvals
        if request.approvals.len() >= config.threshold as usize {
            request.status = if current_time >= request.executable_at {
                RecoveryStatus::Ready
            } else {
                RecoveryStatus::TimeLocked
            };
        }

        Ok(request.status.clone())
    }

    /// Check if a recovery is ready to execute
    pub fn check_recovery_status(
        &mut self,
        recovery_id: &str,
        current_time: u64,
    ) -> Result<RecoveryStatus> {
        let request = self.requests.get_mut(recovery_id)
            .ok_or_else(|| anyhow::anyhow!("Recovery request not found"))?;

        // Update status if timelock has expired
        if request.status == RecoveryStatus::TimeLocked && current_time >= request.executable_at {
            request.status = RecoveryStatus::Ready;
        }

        Ok(request.status.clone())
    }

    /// Execute a recovery request
    pub fn execute_recovery(
        &mut self,
        recovery_id: &str,
        current_time: u64,
    ) -> Result<Vec<u8>> {
        let request = self.requests.get_mut(recovery_id)
            .ok_or_else(|| anyhow::anyhow!("Recovery request not found"))?;

        // Check if recovery is ready
        if request.status != RecoveryStatus::Ready {
            if request.status == RecoveryStatus::TimeLocked && current_time >= request.executable_at {
                request.status = RecoveryStatus::Ready;
            } else {
                bail!("Recovery is not ready to execute");
            }
        }

        // Mark as executed
        request.status = RecoveryStatus::Executed;

        Ok(request.new_public_key.clone())
    }

    /// Cancel a recovery request
    pub fn cancel_recovery(&mut self, recovery_id: &str) -> Result<()> {
        let request = self.requests.get_mut(recovery_id)
            .ok_or_else(|| anyhow::anyhow!("Recovery request not found"))?;

        if request.status == RecoveryStatus::Executed {
            bail!("Cannot cancel an executed recovery");
        }

        request.status = RecoveryStatus::Cancelled;
        Ok(())
    }

    /// Get active recovery requests for an account
    pub fn get_active_recoveries(&self, account_id: &str) -> Vec<&RecoveryRequest> {
        self.requests_by_account
            .get(account_id)
            .map(|request_ids| {
                request_ids.iter()
                    .filter_map(|id| self.requests.get(id))
                    .filter(|req| matches!(
                        req.status,
                        RecoveryStatus::Pending | RecoveryStatus::TimeLocked | RecoveryStatus::Ready
                    ))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Verify a guardian's proof for recovery
    pub async fn verify_guardian_proof(
        &self,
        guardian_commitment: &SocialIdentityCommitment,
        proof: &ZkProof,
        guardians: &[RecoveryGuardian],
    ) -> Result<bool> {
        // Check if the commitment matches any registered guardian
        let is_guardian = guardians.iter().any(|g| {
            g.active &&
            g.commitment.commitment == guardian_commitment.commitment &&
            g.commitment.platform == guardian_commitment.platform
        });

        if !is_guardian {
            return Ok(false);
        }

        // TODO: Actual ZKP verification would happen here
        // For now, we just check that the proof data is non-empty
        Ok(!proof.proof_data.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_commitment(platform: SocialPlatform, id: u8) -> SocialIdentityCommitment {
        let mut commitment = [0u8; 32];
        commitment[0] = id;
        SocialIdentityCommitment {
            commitment,
            platform,
        }
    }

    #[test]
    fn test_initiate_recovery() {
        let mut manager = RecoveryManager::new();
        let config = SocialRecoveryConfig::default();
        let guardian = create_test_commitment(SocialPlatform::Twitter, 1);

        let result = manager.initiate_recovery(
            "account123".to_string(),
            guardian,
            vec![1, 2, 3],
            &config,
            1000,
        );

        assert!(result.is_ok());
        let _recovery_id = result.unwrap();

        let requests = manager.get_active_recoveries("account123");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].status, RecoveryStatus::Pending);
    }

    #[test]
    fn test_recovery_with_threshold() {
        let mut manager = RecoveryManager::new();
        let config = SocialRecoveryConfig {
            threshold: 2,
            ..Default::default()
        };

        let guardian1 = create_test_commitment(SocialPlatform::Twitter, 1);
        let guardian2 = create_test_commitment(SocialPlatform::GitHub, 2);

        // Initiate recovery
        let recovery_id = manager.initiate_recovery(
            "account123".to_string(),
            guardian1,
            vec![1, 2, 3],
            &config,
            1000,
        ).unwrap();

        // Add second approval
        let status = manager.approve_recovery(
            &recovery_id,
            guardian2,
            &config,
            1000,
        ).unwrap();

        assert_eq!(status, RecoveryStatus::TimeLocked);
    }

    #[test]
    fn test_execute_recovery_after_timelock() {
        let mut manager = RecoveryManager::new();
        let config = SocialRecoveryConfig {
            threshold: 1,
            time_lock: 100,
            ..Default::default()
        };

        let guardian = create_test_commitment(SocialPlatform::Discord, 1);

        let recovery_id = manager.initiate_recovery(
            "account123".to_string(),
            guardian,
            vec![4, 5, 6],
            &config,
            1000,
        ).unwrap();

        // Try to execute before timelock
        let result = manager.execute_recovery(&recovery_id, 1050);
        assert!(result.is_err());

        // Execute after timelock
        let result = manager.execute_recovery(&recovery_id, 1101);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![4, 5, 6]);
    }

    #[test]
    fn test_cancel_recovery() {
        let mut manager = RecoveryManager::new();
        let config = SocialRecoveryConfig::default();
        let guardian = create_test_commitment(SocialPlatform::Google, 1);

        let recovery_id = manager.initiate_recovery(
            "account123".to_string(),
            guardian,
            vec![7, 8, 9],
            &config,
            1000,
        ).unwrap();

        // Cancel the recovery
        assert!(manager.cancel_recovery(&recovery_id).is_ok());

        // Try to approve after cancellation
        let result = manager.approve_recovery(
            &recovery_id,
            create_test_commitment(SocialPlatform::Twitter, 2),
            &config,
            1100,
        );
        assert!(result.is_err());
    }
}