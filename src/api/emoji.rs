use serde::Deserialize;

use crate::{Error, api::ApiClient};

#[derive(Debug, Deserialize, Clone)]
pub struct Emoji {
    pub id: String,
    pub name: String,
    pub animated: Option<bool>,
}

impl Emoji {
    pub async fn from_guild(api_client: &ApiClient, guild_id: &str) -> Result<Vec<Self>, Error> {
        let url = format!("{}/guilds/{guild_id}/emojis", api_client.base_url);

        let response = api_client
            .http_client
            .get(url)
            .header("Authorization", &api_client.auth_token)
            .send()
            .await?
            .error_for_status()?;

        Ok(response
            .json::<Vec<Self>>()
            .await
            .map_err(|e| format!("JSON Decoding Error: {e}."))?)
    }
}
