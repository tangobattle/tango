pub mod metered;
pub mod opentok;
pub mod twilio;

#[async_trait::async_trait]
pub trait Backend {
    async fn get(
        &self,
        remote_ip: &std::net::IpAddr,
    ) -> anyhow::Result<Vec<tango_signaling::proto::signaling::packet::hello::IceServer>>;
}
