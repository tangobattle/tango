use async_trait::async_trait;

pub mod openrelay;
pub mod subspace;
pub mod xirsys;

struct Cache {
    pq: std::collections::BinaryHeap<std::cmp::Reverse<(std::time::Instant, std::net::IpAddr)>>,
    values: std::collections::HashMap<std::net::IpAddr, Vec<String>>,
}

pub struct Server {
    backend: Option<Box<dyn Backend + Send + Sync + 'static>>,
    cache: tokio::sync::Mutex<Cache>,
}

impl Server {
    pub fn new(backend: Option<Box<dyn Backend + Send + Sync + 'static>>) -> Self {
        Self {
            backend,
            cache: tokio::sync::Mutex::new(Cache {
                pq: std::collections::BinaryHeap::new(),
                values: std::collections::HashMap::new(),
            }),
        }
    }

    pub async fn get(
        &self,
        remote_ip: std::net::IpAddr,
        _req: tango_protos::relay::GetRequest,
    ) -> Result<tango_protos::relay::GetResponse, anyhow::Error> {
        let backend = if let Some(backend) = self.backend.as_ref() {
            backend
        } else {
            return Ok(tango_protos::relay::GetResponse {
                ice_servers: vec![],
            });
        };

        let now = std::time::Instant::now();
        let mut cache = self.cache.lock().await;

        // Clear out stale cache entries.
        loop {
            let (expires_at, _) = if let Some(std::cmp::Reverse(item)) = cache.pq.peek() {
                item
            } else {
                break;
            };

            if *expires_at > now {
                break;
            }

            let std::cmp::Reverse((_, key)) = cache.pq.pop().unwrap();
            cache.values.remove(&key);
        }

        let ice_servers = match cache.values.entry(remote_ip) {
            std::collections::hash_map::Entry::Occupied(e) => e.get().clone(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let relay_info = backend.get().await?;
                e.insert(relay_info.ice_servers.clone());
                cache
                    .pq
                    .push(std::cmp::Reverse((relay_info.expires_at, remote_ip)));
                relay_info.ice_servers
            }
        };

        Ok(tango_protos::relay::GetResponse { ice_servers })
    }
}

pub struct RelayInfo {
    ice_servers: Vec<String>,
    expires_at: std::time::Instant,
}

#[async_trait]
pub trait Backend {
    async fn get(&self) -> anyhow::Result<RelayInfo>;
}
