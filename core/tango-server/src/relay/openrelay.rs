use async_trait::async_trait;

pub struct Backend {}

impl Backend {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl super::Backend for Backend {
    async fn get(&self) -> anyhow::Result<super::RelayInfo> {
        Ok(super::RelayInfo {
            ice_servers: vec![
                "turn://openrelayproject:openrelayproject@openrelay.metered.ca:80".to_string(),
                "turn://openrelayproject:openrelayproject@openrelay.metered.ca:443".to_string(),
                "turn://openrelayproject:openrelayproject@openrelay.metered.ca:80?transport=tcp"
                    .to_string(),
                "turn://openrelayproject:openrelayproject@openrelay.metered.ca:443?transport=tcp"
                    .to_string(),
                "turns://openrelayproject:openrelayproject@openrelay.metered.ca:443".to_string(),
            ],
            expires_at: std::time::Instant::now() + std::time::Duration::from_secs(60 * 60 * 24),
        })
    }
}
