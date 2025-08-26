use anyhow::Result;
use reqwest::Url;

#[derive(Clone)]
pub(crate) struct ApiServerClient {
    base: Url,
    client: reqwest::Client,
}

impl ApiServerClient {
    pub fn new(base: Url) -> Self {
        Self {
            base,
            client: reqwest::Client::new(),
        }
    }

    pub fn set_path(&mut self, path: &str) -> Self {
        self.base.set_path(path);
        self.clone()
    }

    pub async fn get_logs(&self) -> Result<Vec<String>> {
        #[derive(serde::Deserialize)]
        struct LogsResponse {
            logs: Vec<Vec<String>>,
        }
        let response: LogsResponse = self
            .client
            .get(self.base.clone())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(response.logs.into_iter().flatten().collect())
    }
}
