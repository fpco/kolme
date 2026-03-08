use crate::*;

pub struct Approver<App: KolmeApp> {
    kolme: Kolme<App>,
    secret: SecretKey,
}

impl<App: KolmeApp> Approver<App> {
    pub fn new(kolme: Kolme<App>, secret: SecretKey) -> Self {
        Approver { kolme, secret }
    }

    pub async fn run(self) -> Result<(), KolmeError> {
        let mut new_block = self.kolme.subscribe_new_block();
        self.catch_up_approvals_all().await?;
        loop {
            new_block.listen().await;
            self.catch_up_approvals_all().await?;
        }
    }

    async fn catch_up_approvals_all(&self) -> Result<(), KolmeError> {
        let kolme = self.kolme.read();
        for (chain, _) in kolme.get_bridge_contracts().iter() {
            self.catch_up_approvals(&kolme, chain).await?;
        }
        Ok(())
    }

    async fn catch_up_approvals(
        &self,
        kolme: &KolmeRead<App>,
        chain: ExternalChain,
    ) -> Result<(), KolmeError> {
        let Some((action_id, action)) = kolme.get_next_bridge_action(chain)? else {
            return Ok(());
        };
        let key = self.secret.public_key();
        if action.approvals.contains_key(&key) {
            // We've already approved this action, wait for it to be approved
            // and submitted before approving the next one.
            return Ok(());
        }

        let signature = self.secret.sign_recoverable(&action.payload)?;
        self.kolme
            .sign_propose_await_transaction(
                &self.secret,
                vec![Message::Approve {
                    chain,
                    action_id,
                    signature,
                }],
            )
            .await?;
        Ok(())
    }
}
