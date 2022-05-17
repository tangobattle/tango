use async_trait::async_trait;

pub struct Backend {
    client_id: String,
    client_secret: String,
}

impl Backend {
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self {
            client_id,
            client_secret,
        }
    }
}

#[derive(serde::Serialize)]
struct OAuthTokenRequest {
    client_id: String,
    client_secret: String,
    audience: String,
    grant_type: String,
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    scope: String,
    expires_in: i64,
    token_type: String,
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
struct WebRTCCDNResponseICEServer {
    username: String,
    credential: String,
    urls: String,
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
struct WebRTCCDNResponse {
    ice_servers: Vec<WebRTCCDNResponseICEServer>,
    ttl: i64,
}

#[async_trait]
impl super::Backend for Backend {
    async fn get(&self) -> anyhow::Result<super::RelayInfo> {
        let client = reqwest::Client::new();
        let token_resp = client
            .post("https://subspace.auth0.com/oauth/token")
            .json(&OAuthTokenRequest {
                client_id: self.client_id.clone(),
                client_secret: self.client_secret.clone(),
                audience: "https://api.subspace.com/".to_string(),
                grant_type: "client_credentials".to_string(),
            })
            .send()
            .await?
            .json::<OAuthTokenResponse>()
            .await?;

        let webrtc_resp = client
            .post("https://api.subspace.com/v1/webrtc-cdn")
            .header(
                "Authorizaton",
                format!("{} {}", token_resp.token_type, token_resp.access_token),
            )
            .send()
            .await?
            .json::<WebRTCCDNResponse>()
            .await?;

        Ok(super::RelayInfo {
            ice_servers: webrtc_resp
                .ice_servers
                .into_iter()
                .flat_map(|ice_server| {
                    let (proto, rest) = if let Some(parts) = ice_server.urls.split_once(":") {
                        parts
                    } else {
                        return vec![];
                    };
                    vec![format!(
                        "{}:{}:{}@{}",
                        proto,
                        urlencoding::encode(&ice_server.username),
                        urlencoding::encode(&ice_server.credential),
                        rest
                    )]
                })
                .collect(),
            expires_at: std::time::SystemTime::now()
                + std::time::Duration::from_secs(webrtc_resp.ttl as u64),
        })
    }
}
