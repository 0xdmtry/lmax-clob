use std::future::{ready, Ready};
use std::time::Instant;

use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error,
};
use futures::future::LocalBoxFuture;
use metrics::histogram;

use crate::metrics::HTTP_REQUEST_LATENCY;

pub struct MetricsMiddleware;

impl<S, B> Transform<S, ServiceRequest> for MetricsMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = MetricsMiddlewareService<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(MetricsMiddlewareService { service }))
    }
}

pub struct MetricsMiddlewareService<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for MetricsMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let path = req.path().to_string();
        let method = req.method().to_string();
        let start = Instant::now();
        let fut = self.service.call(req);

        Box::pin(async move {
            let res = fut.await?;
            let elapsed = start.elapsed().as_nanos() as f64;
            histogram!(
                HTTP_REQUEST_LATENCY,
                "path"   => path,
                "method" => method,
                "status" => res.status().as_u16().to_string(),
            )
            .record(elapsed);
            Ok(res)
        })
    }
}
