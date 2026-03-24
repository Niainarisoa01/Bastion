use std::net::IpAddr;
use std::time::Instant;
use dashmap::DashMap;

pub struct RequestContext {
    pub request_id: String,
    pub client_ip: IpAddr,
    pub start_time: Instant,
    pub metadata: DashMap<String, String>,
}

impl RequestContext {
    pub fn new(request_id: String, client_ip: IpAddr) -> Self {
        Self {
            request_id,
            client_ip,
            start_time: Instant::now(),
            metadata: DashMap::new(),
        }
    }
}
