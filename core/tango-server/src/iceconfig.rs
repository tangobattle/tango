use async_trait::async_trait;

pub struct Server {
    backend: Option<Box<dyn Backend + Send + Sync + 'static>>,
}

impl Server {
    pub fn new(backend: Option<Box<dyn Backend + Send + Sync + 'static>>) -> Self {
        Self { backend }
    }

    pub async fn get(
        &self,
        remote_ip: &std::net::IpAddr,
    ) -> Result<tango_protos::iceconfig::GetResponse, anyhow::Error> {
        let backend = if let Some(backend) = self.backend.as_ref() {
            backend
        } else {
            return Ok(tango_protos::iceconfig::GetResponse {
                ice_servers: vec![],
            });
        };

        Ok(tango_protos::iceconfig::GetResponse {
            ice_servers: backend.get(&remote_ip).await?,
        })
    }

    pub async fn get_legacy(
        &self,
        remote_ip: &std::net::IpAddr,
    ) -> Result<tango_protos::iceconfig::GetLegacyResponse, anyhow::Error> {
        let nonlegacy_resp = self.get(remote_ip).await?;

        Ok(tango_protos::iceconfig::GetLegacyResponse {
            ice_servers: nonlegacy_resp
                .ice_servers
                .into_iter()
                .flat_map(|ice_server| {
                    ice_server.urls.into_iter().flat_map({
                        let username = ice_server.username.clone();
                        let credential = ice_server.credential.clone();

                        move |url| {
                            let (proto, rest) = if let Some(parts) = url.split_once(':') {
                                parts
                            } else {
                                return vec![];
                            };

                            if let Some((_, params)) = rest.rsplit_once('?') {
                                if params == "transport=tcp" {
                                    return vec![];
                                }
                            }

                            let username = if let Some(username) = username.clone() {
                                username
                            } else {
                                return vec![format!("{}:{}", proto, rest)];
                            };

                            let credential = if let Some(credential) = credential.clone() {
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
                        }
                    })
                })
                .collect(),
        })
    }
}

#[async_trait]
pub trait Backend {
    async fn get(
        &self,
        remote_ip: &std::net::IpAddr,
    ) -> anyhow::Result<Vec<tango_protos::iceconfig::get_response::IceServer>>;
}
