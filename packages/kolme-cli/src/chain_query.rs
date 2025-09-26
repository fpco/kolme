use anyhow::{anyhow, bail, ensure, Context, Result};
use reqwest::{Client, Url};
use serde::Deserialize;

use crate::RequestBuilderExt;

pub struct ChainApi {
    client: Client,
    url: Url,
}

#[derive(Deserialize)]
pub struct ChainVersion {
    pub code_version: String,
    pub chain_version: String,
    pub next_height: BlockHeight,
}

#[derive(Deserialize)]
pub struct BlockResponse {
    pub code_version: String,
    pub chain_version: String,
}

pub struct ForkInfo {
    first_block: BlockHeight,
    last_block: BlockHeight,
}

#[derive(Deserialize, Copy, Clone)]
struct BlockHeight(u32);

impl BlockHeight {
    pub fn pred(&self) -> Result<BlockHeight> {
        let pred = self.0.checked_sub(1).context("Underflow in pred")?;
        Ok(BlockHeight(pred))
    }

    pub fn middle(&self) -> Result<BlockHeight> {
        let middle = self.0.checked_div(2).context("Underflow in div")?;
        Ok(BlockHeight(middle))
    }

    pub fn increasing_middle(&self, block_height: BlockHeight) -> Result<BlockHeight> {
        ensure!(
            self.0 < block_height.0,
            "Cannot compute middle since current_height is greated"
        );

        let middle = block_height
            .0
            .checked_sub(self.0)
            .context("Overflow in sub")?
            .checked_div(2)
            .context("Underflow in div")?;

        Ok(BlockHeight(middle + self.0))
    }
}

impl ChainApi {
    pub fn new(api_server: Url) -> Result<Self> {
        let client = Client::builder().user_agent("kolme-cli").build()?;
        Ok(ChainApi {
            client,
            url: api_server,
        })
    }

    pub async fn root_info(&self) -> Result<ChainVersion> {
        let response = self
            .client
            .get(self.url.clone())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(response)
    }

    pub async fn block_response(&self, block: BlockHeight) -> Result<BlockResponse> {
        let mut url = self.url.clone();
        url.set_path(&format!("/block/{}", block.0));

        let response = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(response)
    }

    /// Find an arbitrary block height with a particular chain version
    async fn find_block_height(&self, chain_version: &str) -> Result<BlockResponse> {
        let response = self.root_info().await?;
        let mut start_block = BlockHeight(0);
        let mut end_block = response.next_height.pred()?;
        let final_block = end_block;

        loop {
            let response = self.block_response(end_block).await?;
            let result = version_compare::compare(chain_version, response.chain_version.clone())
                .map_err(|err| anyhow!("{err:?}"))?;
            match result {
                version_compare::Cmp::Eq => break Ok(response),
                version_compare::Cmp::Lt => {
                    end_block = end_block.middle()?;
                },
                version_compare::Cmp::Gt => {

                },
                _ => bail!("Impossible case")
            }
        }
    }

    pub async fn fork_info(&self, chain_version: String) -> Result<ForkInfo> {
        todo!()
    }
}
