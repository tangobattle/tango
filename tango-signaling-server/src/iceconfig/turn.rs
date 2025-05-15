pub struct Backend {
    uris: Vec<String>,
}

impl Backend {
    pub fn new(addr: String) -> Self {
        Self {
            uris: vec![format!("turn:{addr}")],
        }
    }
}

#[async_trait::async_trait]
impl super::Backend for Backend {
    async fn get(
        &self,
        _remote_ip: &std::net::IpAddr,
    ) -> anyhow::Result<Vec<tango_signaling::proto::signaling::packet::hello::IceServer>> {
        Ok(vec![tango_signaling::proto::signaling::packet::hello::IceServer {
            credential: None,
            username: None,
            urls: self.uris.clone(),
        }])
    }
}
