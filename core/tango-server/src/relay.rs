pub struct Server {}

impl Server {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn get(
        &self,
        remote_ip: std::net::IpAddr,
        req: tango_protos::relay::GetRequest,
    ) -> Result<tango_protos::relay::GetResponse, anyhow::Error> {
        anyhow::bail!("not implemented")
    }
}
