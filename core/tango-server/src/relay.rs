use async_trait::async_trait;

pub mod subspace;

pub struct Server {
    backend: Option<Box<dyn Backend + Send + Sync + 'static>>,
}

impl Server {
    pub fn new(backend: Option<Box<dyn Backend + Send + Sync + 'static>>) -> Self {
        Self { backend }
    }

    pub async fn get(
        &self,
        remote_ip: std::net::IpAddr,
        req: tango_protos::relay::GetRequest,
    ) -> Result<tango_protos::relay::GetResponse, anyhow::Error> {
        let backend = if let Some(backend) = self.backend.as_ref() {
            backend
        } else {
            return Ok(tango_protos::relay::GetResponse {
                turn_addresses: vec![],
            });
        };
        anyhow::bail!("not implemented")
    }
}

pub struct RelayInfo {
    ice_servers: Vec<String>,
    expires_at: std::time::SystemTime,
}

#[async_trait]
pub trait Backend {
    async fn get(&self) -> anyhow::Result<RelayInfo>;
}
