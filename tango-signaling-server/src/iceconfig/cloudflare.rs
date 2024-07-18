pub struct Backend {
    turn_service_id: String,
    turn_service_api_token: String,
}

impl Backend {
    pub fn new(turn_service_id: String, turn_service_api_token: String) -> Self {
        Self {
            turn_service_id,
            turn_service_api_token,
        }
    }
}

#[derive(serde::Deserialize)]
struct ICEServers {
    #[serde(default)]
    username: Option<String>,

    #[serde(default)]
    credential: Option<String>,

    urls: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    ice_servers: ICEServers,
}

#[derive(serde::Serialize)]
struct Request {
    ttl: u32,
}

#[async_trait::async_trait]
impl super::Backend for Backend {
    async fn get(
        &self,
        _remote_ip: &std::net::IpAddr,
    ) -> anyhow::Result<Vec<tango_signaling::proto::signaling::packet::hello::IceServer>> {
        let client = reqwest::Client::new();

        let resp = client
            .post(format!(
                "https://rtc.live.cloudflare.com/v1/turn/keys/{}/credentials/generate",
                self.turn_service_id
            ))
            .header("Authorization", format!("Bearer {}", self.turn_service_api_token))
            .header("Content-Type", "application/json")
            .json(&Request { ttl: 86400 })
            .send()
            .await?
            .error_for_status()?
            .json::<Response>()
            .await?;

        Ok(resp
            .ice_servers
            .urls
            .into_iter()
            .map(|url| tango_signaling::proto::signaling::packet::hello::IceServer {
                credential: if url.starts_with("stun:") {
                    None
                } else {
                    resp.ice_servers.credential.clone()
                },
                username: if url.starts_with("stun:") {
                    None
                } else {
                    resp.ice_servers.username.clone()
                },
                urls: vec![url],
            })
            .collect())
    }
}
