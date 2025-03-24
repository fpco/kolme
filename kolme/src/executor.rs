use crate::*;

pub struct Executor<App: KolmeApp> {
    kolme: Kolme<App>,
    secret: SecretKey,
}

impl<App: KolmeApp> Executor<App> {
    pub fn new(kolme: Kolme<App>, secret: SecretKey) -> Self {
        Executor { kolme, secret }
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
        for chain in kolme.get_bridge_contracts().keys() {
            self.catch_up_approvals(&kolme, *chain).await?;
        }
        Ok(())
    }

    async fn catch_up_approvals(&self, kolme: &KolmeRead<App>, chain: ExternalChain) -> Result<()> {
        let Some(latest) = kolme.get_latest_action(chain).await? else {
            return Ok(());
        };
        let my_latest = kolme
            .get_latest_approval(chain, self.secret.public_key())
            .await?;
        let mut next = match my_latest {
            None => BridgeActionId::start(),
            Some(latest) => latest.next(),
        };

        while next <= latest {
            self.approve(kolme, chain, next).await?;
            next = next.next();
        }

        Ok(())
    }

    async fn approve(
        &self,
        kolme: &KolmeRead<App>,
        chain: ExternalChain,
        action_id: BridgeActionId,
    ) -> Result<()> {
        let payload = kolme.get_action_payload(chain, action_id).await?;
        let (signature, recovery) = self.secret.sign_recoverable(&payload)?;
        let tx = kolme
            .create_signed_transaction(
                &self.secret,
                vec![Message::Approve {
                    chain,
                    action_id,
                    signature,
                    recovery,
                }],
            )
            .await?;
        self.kolme.propose_transaction(tx)
    }
}
