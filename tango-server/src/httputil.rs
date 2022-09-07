use routerify::ext::RequestExt;

pub struct RealIPGetter {
    use_x_real_ip: bool,
}

impl RealIPGetter {
    pub fn new(use_x_real_ip: bool) -> Self {
        Self { use_x_real_ip }
    }

    pub fn get_remote_real_ip(&self, request: &hyper::Request<hyper::Body>) -> Option<std::net::IpAddr> {
        if !self.use_x_real_ip {
            return Some(request.remote_addr().ip());
        }
        let real_ip_header = request.headers().get("X-Real-IP")?.to_str().ok()?;
        real_ip_header.parse().ok()
    }
}
