pub mod opentok;
pub mod twilio;
pub mod metered;

#[async_trait::async_trait]
pub trait Backend {
    async fn get(
        &self,
        remote_ip: &std::net::IpAddr,
    ) -> anyhow::Result<Vec<tango_protos::matchmaking::packet::hello::IceServer>>;
}
