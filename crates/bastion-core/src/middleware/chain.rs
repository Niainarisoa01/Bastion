use async_trait::async_trait;
use hyper::{Request, Response};
use hyper::body::{Incoming, Bytes};
use http_body_util::combinators::BoxBody;
use std::sync::Arc;

use crate::middleware::context::RequestContext;

pub type ProxyResponse = Response<BoxBody<Bytes, hyper::Error>>;

#[async_trait]
pub trait Middleware: Send + Sync {
    fn name(&self) -> &str;
    fn priority(&self) -> i32 { 0 }
    
    async fn handle(
        &self,
        req: Request<Incoming>,
        ctx: &RequestContext,
        next: Next<'_>,
    ) -> Result<ProxyResponse, Box<dyn std::error::Error + Send + Sync>>;
}

#[async_trait]
pub trait ProxyHandler: Send + Sync {
    async fn call_proxy(
        &self,
        req: Request<Incoming>,
        ctx: &RequestContext,
    ) -> Result<ProxyResponse, Box<dyn std::error::Error + Send + Sync>>;
}

pub struct Next<'a> {
    pub middlewares: &'a [Arc<dyn Middleware>],
    pub final_handler: &'a dyn ProxyHandler,
}

impl<'a> Next<'a> {
    pub async fn run(
        self,
        req: Request<Incoming>,
        ctx: &RequestContext,
    ) -> Result<ProxyResponse, Box<dyn std::error::Error + Send + Sync>> {
        if let Some((first, rest)) = self.middlewares.split_first() {
            let next = Next {
                middlewares: rest,
                final_handler: self.final_handler,
            };
            first.handle(req, ctx, next).await
        } else {
            self.final_handler.call_proxy(req, ctx).await
        }
    }
}

pub struct MiddlewareChain {
    pub(crate) middlewares: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareChain {
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    pub fn add<M: Middleware + 'static>(&mut self, middleware: M) {
        self.middlewares.push(Arc::new(middleware));
        self.sort_by_priority();
    }

    fn sort_by_priority(&mut self) {
        // Higher priority number = runs earlier
        self.middlewares.sort_by_key(|m| -m.priority());
    }
}
