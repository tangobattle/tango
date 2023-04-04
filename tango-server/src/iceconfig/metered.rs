pub struct Backend {
    application_name: String,
    api_key: String,
}

impl Backend {
    pub fn new(application_name: String, api_key: String) -> Self {
        Self {
            application_name,
            api_key,
        }
    }
}

#[derive(serde::Deserialize)]
struct ICEServer {
    username: Option<String>,
    credential: Option<String>,
    urls: Option<String>,
}

#[async_trait::async_trait]
impl super::Backend for Backend {
    async fn get(
        &self,
        _remote_ip: &std::net::IpAddr,
    ) -> anyhow::Result<Vec<tango_protos::matchmaking::packet::hello::IceServer>> {
        let client = reqwest::Client::new();

        let resp = client
            .get(format!(
                "https://{}.metered.live/api/v1/turn/credentials?apiKey={}",
                self.application_name, self.api_key
            ))
            .send()
            .await?
            .error_for_status()?
            .json::<Vec<ICEServer>>()
            .await?;

        Ok(resp
            .into_iter()
            .map(|ice_server| tango_protos::matchmaking::packet::hello::IceServer {
                credential: ice_server.credential,
                username: ice_server.username,
                urls: ice_server.urls.into_iter().collect(),
            })
            .collect())
    }
}
