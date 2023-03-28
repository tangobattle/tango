pub struct Backend {
    username: String,
    credential: String,
}

impl Backend {
    pub fn new(username: String, credential: String) -> Self {
        Self { username, credential }
    }
}

#[async_trait::async_trait]
impl super::Backend for Backend {
    async fn get(
        &self,
        _remote_ip: &std::net::IpAddr,
    ) -> anyhow::Result<Vec<tango_protos::matchmaking::packet::hello::IceServer>> {
        Ok(vec![
            tango_protos::matchmaking::packet::hello::IceServer {
                credential: Some(self.credential.clone()),
                username: Some(self.username.clone()),
                urls: vec!["turn:relay.metered.ca:80".to_string()],
            },
            tango_protos::matchmaking::packet::hello::IceServer {
                credential: Some(self.credential.clone()),
                username: Some(self.username.clone()),
                urls: vec!["turn:relay.metered.ca:443".to_string()],
            },
            tango_protos::matchmaking::packet::hello::IceServer {
                credential: Some(self.credential.clone()),
                username: Some(self.username.clone()),
                urls: vec!["turn:relay.metered.ca:443?transport=tcp".to_string()],
            },
            tango_protos::matchmaking::packet::hello::IceServer {
                credential: None,
                username: None,
                urls: vec!["stun:relay.metered.ca:80".to_string()],
            },
        ])
    }
}
