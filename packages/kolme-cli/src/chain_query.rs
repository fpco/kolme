use anyhow::{anyhow, bail, Context, Result};
use reqwest::{Client, Url};
use serde::Deserialize;

use crate::{RequestBuilderExt, BlockHeight};

pub struct ChainApi {
    client: Client,
    url: Url,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct ChainVersion {
    pub code_version: String,
    pub chain_version: String,
    pub next_height: BlockHeight,
}

#[allow(dead_code)]
pub struct BlockResponse {
    pub code_version: String,
    pub chain_version: String,
    pub block_height: BlockHeight,
}

pub struct ForkInfo {
    pub first_block: BlockHeight,
    pub last_block: BlockHeight,
}

impl ChainApi {
    pub fn new(api_server: Url) -> Result<Self> {
        let client = Client::builder().user_agent("kolme-cli").build()?;
        Ok(ChainApi {
            client,
            url: api_server,
        })
    }

    async fn root_info(&self) -> Result<ChainVersion> {
        let response = self.client.get(self.url.clone()).send_check_json().await?;
        Ok(response)
    }

    async fn block_response(&self, block: BlockHeight) -> Result<BlockResponse> {
        #[derive(Deserialize)]
        struct Response {
            pub code_version: String,
            pub chain_version: String,
        }

        let mut url = self.url.clone();
        url.set_path(&format!("/block/{}", block.0));

        let response: Response = self.client.get(url).send_check_json().await?;
        let response = BlockResponse {
            code_version: response.code_version,
            chain_version: response.chain_version,
            block_height: block,
        };
        Ok(response)
    }

    /// Find an arbitrary block height with a particular chain version
    async fn find_block_height(&self, chain_version: &str) -> Result<BlockResponse> {
        let response = self.root_info().await?;
        let mut start_block = BlockHeight::start();
        let mut end_block = response
            .next_height
            .prev()
            .context("Underflow in pred")?;

        while start_block.0 <= end_block.0 {
            let middle_block = start_block.increasing_middle(end_block)?;
            let response = self.block_response(middle_block).await?;
            let result = version_compare::compare(chain_version, &response.chain_version)
                .map_err(|err| anyhow!("{err:?}"))?;

            match result {
                version_compare::Cmp::Eq => return Ok(response),
                version_compare::Cmp::Lt => {
                    // The version we want is older than the one at `middle_block`.
                    // Search in the lower half.
                    if middle_block.is_start() {
                        // We are at the beginning and the version is still too high.
                        bail!(
                            "Chain version {} not found, earliest is {}",
                            chain_version,
                            response.chain_version
                        );
                    }
                    end_block = middle_block.prev().context("Underflow in pred")?;
                }
                version_compare::Cmp::Gt => {
                    // The version we want is newer than the one at `middle_block`.
                    // Search in the upper half.
                    start_block = middle_block.next();
                }
                _ => bail!("Impossible case"),
            }
        }

        bail!("Could not find a block with chain version {chain_version}");
    }

    /// Find the first block height with a particular chain version
    async fn find_first_block(
        &self,
        chain_version: &str,
        mut end_block: BlockHeight,
    ) -> Result<BlockHeight> {
        let mut start_block = BlockHeight::start();
        let mut first_block = None;

        while start_block.0 <= end_block.0 {
            let middle_block = start_block.increasing_middle(end_block)?;
            let response = self.block_response(middle_block).await?;

            if response.chain_version == chain_version {
                first_block = Some(middle_block);
                if middle_block.is_start() {
                    break;
                }
                end_block = middle_block.prev().context("Underflow in pred")?;
            } else if version_compare::compare(&response.chain_version, chain_version)
                .map_err(|err| anyhow!("{err:?}"))?
                == version_compare::Cmp::Lt
            {
                start_block = middle_block.next();
            } else {
                if middle_block.is_start() {
                    break;
                }
                end_block = middle_block.prev().context("Underflow in pred")?;
            }
        }

        first_block.context(format!(
            "Could not find first block for chain version {chain_version}"
        ))
    }

    /// Find the last block height with a particular chain version
    async fn find_last_block(
        &self,
        chain_version: &str,
        mut start_block: BlockHeight,
    ) -> Result<BlockHeight> {
        let latest_block = self
            .root_info()
            .await?
            .next_height
            .prev()
            .context("Underflow in pred")?;
        let mut end_block = latest_block;
        let mut last_block = None;

        while start_block.0 <= end_block.0 {
            let middle_block = start_block.increasing_middle(end_block)?;
            let response = self.block_response(middle_block).await?;

            if response.chain_version == chain_version {
                last_block = Some(middle_block);
                start_block = middle_block.next();
            } else if version_compare::compare(&response.chain_version, chain_version)
                .map_err(|err| anyhow!("{err:?}"))?
                == version_compare::Cmp::Gt
            {
                if middle_block.is_start() {
                    break;
                }
                end_block = middle_block.prev().context("Underflow in pred")?;
            } else {
                start_block = middle_block.next();
            }
        }

        last_block.context(format!(
            "Could not find last block for chain version {chain_version}"
        ))
    }

    pub async fn fork_info(&self, chain_version: String) -> Result<ForkInfo> {
        let found_block = self.find_block_height(&chain_version).await?;

        let first_block = self
            .find_first_block(&chain_version, found_block.block_height)
            .await?;

        let last_block = self
            .find_last_block(&chain_version, found_block.block_height)
            .await?;

        Ok(ForkInfo {
            first_block,
            last_block,
        })
    }
}
