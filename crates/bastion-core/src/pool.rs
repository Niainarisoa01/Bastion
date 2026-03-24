use hyper::body::Incoming;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use std::time::Duration;

#[derive(Clone)]
pub struct PoolManager {
    pub client: Client<HttpConnector, Incoming>,
}

impl Default for PoolManager {
    fn default() -> Self {
        Self::new(Duration::from_secs(90), 100)
    }
}

impl PoolManager {
    pub fn new(idle_timeout: Duration, max_idle_per_host: usize) -> Self {
        let mut connector = HttpConnector::new();
        connector.enforce_http(false);
        
        let client = Client::builder(TokioExecutor::new())
            .pool_idle_timeout(idle_timeout)
            .pool_max_idle_per_host(max_idle_per_host)
            .build(connector);

        Self { client }
    }
}
