use crate::*;

/// A component that ensures a proposal is sent to upgrade the chain, if needed.
pub struct Upgrader<App: KolmeApp> {
    kolme: Kolme<App>,
    public: PublicKey,
    secret: SecretKey,
    desired_version: String,
}

impl<App: KolmeApp> Upgrader<App> {
    pub fn new(kolme: Kolme<App>, secret: SecretKey, desired_version: impl Into<String>) -> Self {
        Upgrader {
            kolme,
            public: secret.public_key(),
            secret,
            desired_version: desired_version.into(),
        }
    }

    pub async fn run(self) {
        loop {
            if let Err(e) = self.run_inner().await {
                tracing::error!("Unexpected error in Upgrader loop: {e}");
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }
    }

    async fn run_inner(&self) -> Result<()> {
        let mut new_block = self.kolme.subscribe_new_block();
        loop {
            if let Err(e) = self.run_single().await {
                tracing::error!("Error in Upgrader::run_single, pausing and trying again: {e}");
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }

            new_block.listen().await;
        }
    }

    async fn run_single(&self) -> Result<()> {
        let kolme = self.kolme.read();
        let framework_state = kolme.get_framework_state();

        // Are we already on the desired version? If so, nothing to be done.
        if kolme.get_chain_version() == &self.desired_version {
            return Ok(());
        }

        // If we're not in the validator set, we can't do anything
        if let Err(e) = framework_state
            .get_validator_set()
            .ensure_is_validator(self.public)
        {
            tracing::warn!(
                "Upgrader has public key {} which is not in the validator set: {e}",
                self.public
            );
            return Ok(());
        }

        tracing::info!(
            "Total Proposals: {}",
            framework_state.get_admin_proposal_state().proposals.len()
        );

        // Check if there's an existing upgrade proposal for our version
        for (id, proposal) in &framework_state.get_admin_proposal_state().proposals {
            if let ProposalPayload::Upgrade(upgrade) = &proposal.payload {
                if upgrade.as_inner().desired_version != self.desired_version {
                    continue;
                }
            }

            // OK, we've found a proposal, check if we've already voted on it.
            if proposal.approvals.contains_key(&self.public) {
                tracing::info!("Upgrader {} has already voted", self.public);
                return Ok(());
            }

            // OK, we didn't vote on it. Time to vote!
            tracing::info!(
                "Voting to approve upgrade proposal {id} with public key {}",
                self.public
            );

            self.vote_on_upgrade(*id, &proposal.payload).await?;
            return Ok(());
        }

        // No matching upgrade proposal found and we're on the wrong version.
        // Time for us to propose the upgrade!
        tracing::info!("Proposing Upgrade");
        self.propose_upgrade().await?;
        tracing::info!("Successfully proposed Upgrade");

        Ok(())
    }

    async fn vote_on_upgrade(&self, id: AdminProposalId, payload: &ProposalPayload) -> Result<()> {
        self.kolme
            .sign_propose_await_transaction(
                &self.secret,
                vec![Message::Admin(AdminMessage::approve(
                    id,
                    payload,
                    &self.secret,
                )?)],
            )
            .await?;
        Ok(())
    }

    async fn propose_upgrade(&self) -> Result<()> {
        self.kolme
            .sign_propose_await_transaction(
                &self.secret,
                vec![Message::Admin(AdminMessage::upgrade(
                    &self.desired_version,
                    &self.secret,
                )?)],
            )
            .await?;
        Ok(())
    }
}
