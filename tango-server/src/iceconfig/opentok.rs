pub struct Backend {
    opentok: opentok_server::OpenTok,
}

impl Backend {
    pub fn new(api_key: String, api_secret: String) -> Self {
        Self {
            opentok: opentok_server::OpenTok::new(api_key, api_secret),
        }
    }
}

#[derive(serde::Deserialize)]
struct ICEServer {
    #[serde(default)]
    username: Option<String>,

    #[serde(default)]
    credential: Option<String>,

    url: String,
}

#[derive(serde::Deserialize)]
struct Session {
    ice_servers: Vec<ICEServer>,
}

#[async_trait::async_trait]
impl super::Backend for Backend {
    async fn get(
        &self,
        remote_ip: &std::net::IpAddr,
    ) -> anyhow::Result<Vec<tango_net::proto::signaling::packet::hello::IceServer>> {
        let opentok_session_id = self
            .opentok
            .create_session(opentok_server::SessionOptions {
                location: Some(&remote_ip.to_string()),
                ..Default::default()
            })
            .await?;

        let token = self
            .opentok
            .generate_token(&opentok_session_id, opentok_server::TokenRole::Publisher);

        let client = reqwest::Client::new();

        let resp = client
            .get(format!("https://api.opentok.com/session/{}", opentok_session_id))
            .header("X-OPENTOK-AUTH", token)
            .header("Accept", "application/json")
            .send()
            .await?
            .error_for_status()?
            .json::<Vec<Session>>()
            .await?;

        let session = if let Some(session) = resp.into_iter().next() {
            session
        } else {
            anyhow::bail!("no session returned");
        };

        Ok(session
            .ice_servers
            .into_iter()
            .map(|ice_server| tango_net::proto::signaling::packet::hello::IceServer {
                credential: ice_server.credential,
                username: ice_server.username,
                urls: vec![ice_server.url],
            })
            .collect())
    }
}
