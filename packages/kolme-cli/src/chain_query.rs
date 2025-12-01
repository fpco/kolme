use anyhow::{Context, Result};
use kolme::core::BlockHeight;
use reqwest::{Client, Url};
use serde::Deserialize;

use crate::RequestBuilderExt;

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

#[derive(Deserialize, Debug)]
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

    #[allow(dead_code)]
    async fn root_info(&self) -> Result<ChainVersion> {
        let response = self.client.get(self.url.clone()).send_check_json().await?;
        Ok(response)
    }

    pub async fn fork_info(&self, chain_version: String) -> Result<ForkInfo> {
        let mut url = self.url.clone();
        url.set_path("/fork-info");
        url.query_pairs_mut()
            .append_pair("chain_version", &chain_version);

        let fork_info: ForkInfo = self.client.get(url).send_check_json().await?;
        Ok(fork_info)
    }
}
