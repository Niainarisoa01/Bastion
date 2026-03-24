use async_trait::async_trait;
use hyper::{Request, Response, StatusCode};
use hyper::body::{Incoming, Bytes};
use http_body_util::{BodyExt, Full};
use std::net::IpAddr;
use ipnet::IpNet;

use crate::middleware::chain::{Middleware, Next, ProxyResponse};
use crate::middleware::context::RequestContext;

#[derive(Clone, Debug)]
pub enum IpFilterMode {
    Whitelist,
    Blacklist,
}

#[derive(Clone, Debug)]
pub struct IpFilterConfig {
    pub mode: IpFilterMode,
    pub rules: Vec<String>, // IP addresses or CIDR notations
}

pub struct IpFilterMiddleware {
    mode: IpFilterMode,
    networks: Vec<IpNet>,
    exact_ips: Vec<IpAddr>,
}

impl IpFilterMiddleware {
    pub fn new(config: IpFilterConfig) -> Self {
        let mut networks = Vec::new();
        let mut exact_ips = Vec::new();

        for rule in &config.rules {
            if let Ok(net) = rule.parse::<IpNet>() {
                networks.push(net);
            } else if let Ok(ip) = rule.parse::<IpAddr>() {
                exact_ips.push(ip);
            } else {
                tracing::warn!("Invalid IP filter rule: {}", rule);
            }
        }

        Self {
            mode: config.mode,
            networks,
            exact_ips,
        }
    }

    fn ip_matches(&self, ip: &IpAddr) -> bool {
        if self.exact_ips.contains(ip) {
            return true;
        }
        self.networks.iter().any(|net| net.contains(ip))
    }
}

#[async_trait]
impl Middleware for IpFilterMiddleware {
    fn name(&self) -> &str {
        "ip_filter"
    }

    fn priority(&self) -> i32 {
        99 // Run very early, right after rate limiter
    }

    async fn handle(
        &self,
        req: Request<Incoming>,
        ctx: &RequestContext,
        next: Next<'_>,
    ) -> Result<ProxyResponse, Box<dyn std::error::Error + Send + Sync>> {
        let client_ip = &ctx.client_ip;
        let matched = self.ip_matches(client_ip);

        let allowed = match self.mode {
            IpFilterMode::Whitelist => matched,
            IpFilterMode::Blacklist => !matched,
        };

        if !allowed {
            tracing::warn!(ip = %client_ip, "IP blocked by filter");
            let body = Full::new(Bytes::from("403 Forbidden - IP not allowed\n"))
                .map_err(|never| match never {})
                .boxed();
            let mut res = Response::new(body);
            *res.status_mut() = StatusCode::FORBIDDEN;
            return Ok(res);
        }

        next.run(req, ctx).await
    }
}
