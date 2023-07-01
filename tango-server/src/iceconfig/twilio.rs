use hmac::Mac;
use jwt::SignWithKey;

pub struct Backend {
    account_sid: String,
    api_sid: String,
    api_secret: String,
}

impl Backend {
    pub fn new(account_sid: String, api_sid: String, api_secret: String) -> Self {
        Self {
            account_sid,
            api_sid,
            api_secret,
        }
    }
}

#[derive(serde::Deserialize)]
struct ICEServer {
    username: String,
    credential: String,
    urls: String,
}

#[derive(serde::Deserialize)]
struct ResponseVideoNetworkTraversalService {
    ice_servers: Vec<ICEServer>,
}

#[derive(serde::Deserialize)]
struct ResponseVideo {
    network_traversal_service: ResponseVideoNetworkTraversalService,
}

#[derive(serde::Deserialize)]
struct Response {
    video: ResponseVideo,
}

#[derive(serde::Serialize)]
struct VideoGrants {}

#[derive(serde::Serialize)]
struct Grants {
    video: VideoGrants,
}

#[derive(serde::Serialize)]
struct Claims {
    jti: String,
    grants: Grants,
    iat: u64,
    exp: u64,
    iss: String,
    sub: String,
}

#[derive(serde::Serialize)]
struct Header {
    alg: String,
    typ: String,
    cty: String,
}

impl jwt::JoseHeader for Header {
    fn algorithm_type(&self) -> jwt::AlgorithmType {
        jwt::AlgorithmType::Hs256
    }
}

#[async_trait::async_trait]
impl super::Backend for Backend {
    async fn get(
        &self,
        _remote_ip: &std::net::IpAddr,
    ) -> anyhow::Result<Vec<tango_net::proto::signaling::packet::hello::IceServer>> {
        let key = hmac::Hmac::<sha2::Sha256>::new_from_slice(&self.api_secret.as_bytes())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        let claims = Claims {
            jti: format!("{}-{}", self.api_sid, now),
            grants: Grants { video: VideoGrants {} },
            iat: now,
            exp: now + 3600,
            iss: self.api_sid.clone(),
            sub: self.account_sid.clone(),
        };

        let header = Header {
            alg: "HS256".to_string(),
            typ: "JWT".to_string(),
            cty: "twilio-fpa;v=1".to_string(),
        };

        let token = jwt::Token::new(header, claims).sign_with_key(&key)?;

        let client = reqwest::Client::new();

        let resp = client
            .post("https://ecs.us1.twilio.com/v1/Configuration")
            .header("X-Twilio-Token", token.as_str())
            .form(&{
                let mut params = std::collections::HashMap::new();
                params.insert("service", "video");
                params.insert("sdk_version", "2.5.0");
                params
            })
            .send()
            .await?
            .error_for_status()?
            .json::<Response>()
            .await?;

        let mut ice_servers = resp
            .video
            .network_traversal_service
            .ice_servers
            .into_iter()
            .map(|ice_server| tango_net::proto::signaling::packet::hello::IceServer {
                credential: Some(ice_server.credential),
                username: Some(ice_server.username),
                urls: vec![ice_server.urls],
            })
            .collect::<Vec<_>>();
        ice_servers.push(tango_net::proto::signaling::packet::hello::IceServer {
            credential: None,
            username: None,
            urls: vec!["stun:global.stun.twilio.com:3478".to_string()],
        });
        Ok(ice_servers)
    }
}
