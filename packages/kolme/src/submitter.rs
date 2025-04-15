use std::collections::{HashMap, HashSet};

use crate::*;

/// Component which submits necessary transactions to the blockchain.
pub struct Submitter<App: KolmeApp> {
    kolme: Kolme<App>,
    args: ChainArgs,
    /// Keep track of which genesis contracts we've already created.
    ///
    /// Without this, we almost always end up double-instantiating the first contract.
    /// Reason: we immediately instantiate a contract, then see the new block
    /// for the genesis transaction, and then try to instantiate it again
    /// because our new contract will only be recognized in a later transaction.
    ///
    /// Simple solution: only instantiate once per chain.
    genesis_created: HashSet<ExternalChain>,
    last_submitted: HashMap<ExternalChain, BridgeActionId>,
}

enum ChainArgs {
    Cosmos {
        seed_phrase: cosmos::SeedPhrase,
    },
    Solana {
        keypair: kolme_solana_bridge_client::keypair::Keypair,
    },
}

impl ChainArgs {
    #[inline]
    fn can_handle(&self, chain: ExternalChain) -> bool {
        match self {
            Self::Cosmos { .. } => chain.to_cosmos_chain().is_some(),
            Self::Solana { .. } => chain.to_solana_chain().is_some(),
        }
    }
}

impl<App: KolmeApp> Submitter<App> {
    pub fn new_cosmos(kolme: Kolme<App>, seed_phrase: cosmos::SeedPhrase) -> Self {
        Submitter {
            kolme,
            args: ChainArgs::Cosmos { seed_phrase },
            last_submitted: HashMap::new(),
            genesis_created: HashSet::new(),
        }
    }

    pub fn new_solana(
        kolme: Kolme<App>,
        keypair: kolme_solana_bridge_client::keypair::Keypair,
    ) -> Self {
        Submitter {
            kolme,
            args: ChainArgs::Solana { keypair },
            last_submitted: HashMap::new(),
            genesis_created: HashSet::new(),
        }
    }

    pub async fn run(mut self) -> Result<()> {
        let chains = self
            .kolme
            .read()
            .await
            .get_bridge_contracts()
            .keys()
            .copied()
            .filter(|x| self.args.can_handle(*x))
            .collect::<Vec<_>>();

        if chains.is_empty() {
            return Ok(());
        }

        let mut receiver = self.kolme.subscribe();
        self.submit_zero_or_one(&chains).await?;
        tracing::info!("Submitter has caught up, waiting for new events.");

        loop {
            match receiver.recv().await? {
                Notification::NewBlock(_) => (),
                Notification::GenesisInstantiation {
                    chain: _,
                    contract: _,
                } => continue,
                Notification::Broadcast { tx: _ } => continue,
            }
            self.submit_zero_or_one(&chains).await?;
        }
    }

    /// Submit 0 transactions (if nothing is needed) or the next event's transactions.
    ///
    /// We only do 0 or 1, since we always wait for listeners to confirm that our actions succeeded before continuing.
    async fn submit_zero_or_one(&mut self, chains: &[ExternalChain]) -> Result<()> {
        // TODO we can probably unify genesis and other actions into a single per-chain feed
        let genesis_action = self.kolme.read().await.get_next_genesis_action();
        if let Some(genesis_action) = genesis_action {
            return self.handle_genesis(genesis_action).await;
        }

        for chain in chains {
            if let Some(bridge_action) = self
                .kolme
                .read()
                .await
                .get_next_bridge_action(*chain)
                .await?
            {
                return self.handle_bridge_action(bridge_action).await;
            }
        }

        Ok(())
    }

    async fn handle_genesis(&mut self, genesis_action: GenesisAction) -> Result<()> {
        let (contract_addr, chain) = match genesis_action {
            GenesisAction::InstantiateCosmos {
                chain,
                code_id,
                args,
            } => {
                let ChainArgs::Cosmos { seed_phrase } = &self.args else {
                    return Ok(());
                };

                if self.genesis_created.contains(&chain.into()) {
                    return Ok(());
                }

                let cosmos = self.kolme.read().await.get_cosmos(chain).await?;

                let addr =
                    cosmos_submitter::instantiate(&cosmos, seed_phrase, code_id, args).await?;

                (addr, chain.into())
            }
            GenesisAction::InstantiateSolana {
                chain,
                program_id,
                args,
            } => {
                let ChainArgs::Solana { keypair } = &self.args else {
                    return Ok(());
                };

                if self.genesis_created.contains(&chain.into()) {
                    return Ok(());
                }

                let client = self.kolme.read().await.get_solana_client(chain).await;

                solana_submitter::instantiate(&client, &keypair, &program_id, args).await?;

                (program_id, chain.into())
            }
        };

        self.kolme
            .notify_genesis_instantiation(chain, contract_addr);
        self.genesis_created.insert(chain);

        Ok(())
    }

    async fn handle_bridge_action(
        &mut self,
        PendingBridgeAction {
            chain,
            payload,
            height,
            message: msg_index,
            action_id,
        }: PendingBridgeAction,
    ) -> Result<()> {
        if let Some(last) = self.last_submitted.get(&chain) {
            if *last <= action_id {
                return Ok(());
            }
        }

        let block = self.kolme.read().await.load_block(height).await?;
        let message = block
            .0
            .message
            .as_inner()
            .tx
            .0
            .message
            .as_inner()
            .messages
            .get(msg_index)
            .with_context(|| format!("Block height {height} is missing message #{msg_index}"))?;

        let Message::ProcessorApprove {
            chain: chain2,
            action_id: action_id2,
            processor,
            approvers,
        } = message
        else {
            anyhow::bail!("Wrong message type for {height}#{msg_index}");
        };

        anyhow::ensure!(&chain == chain2);
        anyhow::ensure!(&action_id == action_id2);

        let contract = {
            let kolme = self.kolme.read().await;
            match kolme.get_bridge_contracts().get(&chain) {
                None => return Ok(()),
                Some(config) => match &config.bridge {
                    BridgeContract::NeededCosmosBridge { .. }
                    | BridgeContract::NeededSolanaBridge { .. } => return Ok(()),
                    BridgeContract::Deployed(contract) => contract.clone(),
                },
            }
        };

        let tx_hash = match &self.args {
            ChainArgs::Cosmos { seed_phrase } => {
                let Some(cosmos_chain) = chain.to_cosmos_chain() else {
                    return Ok(());
                };

                let cosmos = self.kolme.read().await.get_cosmos(cosmos_chain).await?;

                cosmos_submitter::execute(
                    &cosmos,
                    seed_phrase,
                    &contract,
                    *processor,
                    approvers.clone(),
                    payload,
                )
                .await?
            }
            ChainArgs::Solana { keypair } => {
                let Some(solana_chain) = chain.to_solana_chain() else {
                    return Ok(());
                };

                let client = self
                    .kolme
                    .read()
                    .await
                    .get_solana_client(solana_chain)
                    .await;

                solana_submitter::execute(
                    &client,
                    keypair,
                    &contract,
                    *processor,
                    approvers.clone(),
                    payload,
                )
                .await?
            }
        };

        tracing::info!(
            "Transaction submitted for {chain:?}#{action_id}: {}",
            tx_hash
        );
        self.last_submitted.insert(chain, action_id);

        Ok(())
    }
}

mod cosmos_submitter {
    use cosmos::{Cosmos, HasAddressHrp, SeedPhrase, TxBuilder};
    use shared::cosmos::{ExecuteMsg, InstantiateMsg};

    use super::*;

    pub async fn instantiate(
        cosmos: &Cosmos,
        seed_phrase: &SeedPhrase,
        code_id: u64,
        args: InstantiateArgs,
    ) -> Result<String> {
        let needed_approvers = u16::try_from(args.needed_approvers)?;
        let msg = InstantiateMsg {
            processor: args.processor,
            approvers: args.approvers,
            needed_approvers,
        };

        let wallet = seed_phrase.with_hrp(cosmos.get_address_hrp())?;
        let contract = cosmos
            .make_code_id(code_id)
            .instantiate(
                &wallet,
                "Kolme Framework Bridge Contract".to_owned(),
                vec![],
                &msg,
                cosmos::ContractAdmin::Sender,
            )
            .await?;

        tracing::info!("Instantiate new contract: {contract}");

        let res = TxBuilder::default()
            .add_update_contract_admin(&contract, &wallet, &contract)
            .sign_and_broadcast(&cosmos, &wallet)
            .await?;

        tracing::info!(
            "Updated admin on {contract} to its own address in tx {}",
            res.txhash
        );

        Ok(contract.to_string())
    }

    pub async fn execute(
        cosmos: &Cosmos,
        seed_phrase: &SeedPhrase,
        contract: &str,
        processor: SignatureWithRecovery,
        approvers: Vec<SignatureWithRecovery>,
        payload: String,
    ) -> Result<String> {
        let msg = ExecuteMsg::Signed {
            processor,
            approvers,
            payload,
        };

        let contract = cosmos.make_contract(contract.parse()?);
        let tx = contract
            .execute(
                &seed_phrase.with_hrp(contract.get_address_hrp())?,
                vec![],
                msg,
            )
            .await?;

        Ok(tx.txhash)
    }
}

mod solana_submitter {
    use std::{ops::Deref, str::FromStr};

    use base64::Engine;
    use borsh::BorshDeserialize;
    use kolme_solana_bridge_client::{
        init_tx, instruction::account_meta::AccountMeta, keypair::Keypair, pubkey::Pubkey,
        signed_tx, InitializeIxData, Payload, Secp256k1PubkeyCompressed, Secp256k1Signature,
        Signature, SignedMsgIxData,
    };

    use super::*;

    pub async fn instantiate(
        client: &SolanaClient,
        keypair: &Keypair,
        program_id: &str,
        args: InstantiateArgs,
    ) -> Result<()> {
        let needed_approvers = u8::try_from(args.needed_approvers)?;
        let mut executors = Vec::with_capacity(args.approvers.len());

        for a in args.approvers {
            executors.push(Secp256k1PubkeyCompressed(a.as_bytes().deref().try_into()?));
        }

        let data = InitializeIxData {
            needed_executors: needed_approvers,
            processor: Secp256k1PubkeyCompressed(args.processor.as_bytes().deref().try_into()?),
            executors,
        };

        let program_pubkey = Pubkey::from_str(&program_id)?;
        let blockhash = client.get_latest_blockhash().await?;
        let tx =
            init_tx(program_pubkey, blockhash, &keypair, &data).map_err(|x| anyhow::anyhow!(x))?;

        client.send_and_confirm_transaction(&tx).await?;

        Ok(())
    }

    pub async fn execute(
        client: &SolanaClient,
        keypair: &Keypair,
        program_id: &str,
        processor: SignatureWithRecovery,
        approvers: Vec<SignatureWithRecovery>,
        payload: String,
    ) -> Result<String> {
        let payload_bytes = base64::engine::general_purpose::STANDARD.decode(&payload)?;
        let payload: Payload = BorshDeserialize::try_from_slice(&payload_bytes)
            .map_err(|x| anyhow::anyhow!("Error deserializing Solana bridge payload: {:?}", x))?;

        let program_id = Pubkey::from_str(&program_id)?;
        let mut metas: Vec<AccountMeta> = Vec::with_capacity(1 + payload.accounts.len());
        metas.push(AccountMeta {
            pubkey: Pubkey::new_from_array(payload.program_id),
            is_writable: true,
            is_signer: false,
        });

        metas.extend(payload.accounts.iter().map(|x| AccountMeta {
            pubkey: Pubkey::new_from_array(x.pubkey),
            is_writable: x.is_writable,
            is_signer: false,
        }));

        let mut executors = Vec::with_capacity(approvers.len());

        for a in approvers {
            let sig = Signature {
                signature: Secp256k1Signature(a.sig.to_bytes().deref().try_into()?),
                recovery_id: processor.recid.to_byte(),
            };

            executors.push(sig);
        }

        let data = SignedMsgIxData {
            processor: Signature {
                signature: Secp256k1Signature(processor.sig.to_bytes().deref().try_into()?),
                recovery_id: processor.recid.to_byte(),
            },
            executors,
            payload: payload_bytes,
        };

        let blockhash = client.get_latest_blockhash().await?;
        let tx = signed_tx(program_id, blockhash, &keypair, &data, &metas)
            .map_err(|x| anyhow::anyhow!(x))?;

        let sig = client.send_and_confirm_transaction(&tx).await?;

        Ok(sig.to_string())
    }
}
