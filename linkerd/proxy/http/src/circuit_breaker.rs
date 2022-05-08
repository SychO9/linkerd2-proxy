use http::{self};
use linkerd_stack::{layer, NewService};
use std::{
    task::{Context, Poll},
};
use futures::{future, prelude::*};
use tracing::{info};

#[derive(Clone, Debug)]
pub struct NewCircuitBreaker<M> {
    inner: M,
}

#[derive(Clone, Debug)]
pub struct CircuitBreaker<S> {
    inner: S,
}

// === impl NewCircuitBreaker ===

impl<N> NewCircuitBreaker<N> {
    pub fn layer() -> impl layer::Layer<N, Service = Self> + Clone {
        layer::mk(move |inner| Self {
            inner,
        })
    }
}

impl<T, M> NewService<T> for NewCircuitBreaker<M>
    where
        M: NewService<T>,
{
    type Service = CircuitBreaker<M::Service>;

    #[inline]
    fn new_service(&self, t: T) -> Self::Service {
        CircuitBreaker {
            inner: self.inner.new_service(t),
        }
    }
}

// === impl Service ===

impl<S, A, B> tower::Service<http::Request<A>> for CircuitBreaker<S>
    where
        S: tower::Service<http::Request<A>, Response = http::Response<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = future::MapOk<S::Future, fn(S::Response) -> S::Response>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<A>) -> Self::Future {
        info!("[MM]: {:?}", req.uri());
        info!("[MM]: {:?}", req.headers());

        let future = self.inner.call(req);

        future.map_ok(|res: http::Response<B>| {
            info!("[MM]: {:?}", res.status());

            res
        })
    }
}
