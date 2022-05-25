use async_trait::async_trait;

pub struct Backend {
    ident: String,
    secret: String,
}

impl Backend {
    pub fn new(ident: String, secret: String) -> Self {
        Self { ident, secret }
    }
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
struct TurnResponseVICEServers {
    #[serde(default)]
    username: Option<String>,

    #[serde(default)]
    credential: Option<String>,

    url: String,
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
struct TurnResponseV {
    #[serde(rename = "iceServers")]
    ice_servers: Vec<TurnResponseVICEServers>,
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
struct TurnResponse {
    v: TurnResponseV,
    s: String,
}

#[async_trait]
impl super::Backend for Backend {
    async fn get(&self) -> anyhow::Result<super::RelayInfo> {
        let client = reqwest::Client::new();

        let mut url = "https://global.xirsys.net/_turn/tango".parse::<url::Url>()?;
        url.set_username(&self.ident).unwrap();
        url.set_password(Some(&self.secret)).unwrap();

        let resp = client
            .put(url)
            .send()
            .await?
            .error_for_status()?
            .json::<TurnResponse>()
            .await?;

        if resp.s != "ok" {
            anyhow::bail!("got non-ok response");
        }

        Ok(super::RelayInfo {
            ice_servers: resp
                .v
                .ice_servers
                .into_iter()
                .flat_map(|ice_server| {
                    let (proto, rest) = if let Some(parts) = ice_server.url.split_once(':') {
                        parts
                    } else {
                        return vec![];
                    };

                    let username = if let Some(username) = ice_server.username {
                        username
                    } else {
                        return vec![format!("{}:{}", proto, rest)];
                    };

                    let credential = if let Some(credential) = ice_server.credential {
                        credential
                    } else {
                        return vec![format!(
                            "{}:{}@{}",
                            proto,
                            urlencoding::encode(&username),
                            rest
                        )];
                    };

                    vec![format!(
                        "{}:{}:{}@{}",
                        proto,
                        urlencoding::encode(&username),
                        urlencoding::encode(&credential),
                        rest
                    )]
                })
                .collect(),
            expires_at: std::time::Instant::now() + std::time::Duration::from_secs(10),
        })
    }
}
