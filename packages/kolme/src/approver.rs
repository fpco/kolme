use crate::*;

pub struct Approver<App: KolmeApp> {
    kolme: Kolme<App>,
    secret: SecretKey,
}

impl<App: KolmeApp> Approver<App> {
    pub fn new(kolme: Kolme<App>, secret: SecretKey) -> Self {
        Approver { kolme, secret }
    }

    pub async fn run(self) -> Result<()> {
        let mut receiver = self.kolme.subscribe();
        self.catch_up_approvals_all().await?;
        loop {
            if let Notification::NewBlock(_) = receiver.recv().await? {
                self.catch_up_approvals_all().await?;
            }
        }
    }

    async fn catch_up_approvals_all(&self) -> Result<()> {
        let kolme = self.kolme.read().await;
        for (chain, _) in kolme.get_bridge_contracts().iter() {
            self.catch_up_approvals(&kolme, chain).await?;
        }
        Ok(())
    }

    async fn catch_up_approvals(&self, kolme: &KolmeRead<App>, chain: ExternalChain) -> Result<()> {
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
        let tx = kolme
            .create_signed_transaction(
                &self.secret,
                vec![Message::Approve {
                    chain,
                    action_id,
                    signature,
                }],
            )
            .await?;
        self.kolme.propose_transaction(tx)
    }
}
