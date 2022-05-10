use http::{self};
use linkerd_stack::{layer, NewService};
use std::{
    fmt,
    task::{Context, Poll},
    pin::Pin,
};
use futures::{prelude::*};
use tracing::{info};
use std::collections::HashMap;
use pin_project::pin_project;

type MetricsMap = &'static mut HashMap<String, DestinationMetrics>;

#[derive(Clone, Debug)]
pub struct NewCircuitBreaker<M> {
    inner: M,
}

#[derive(Clone, Debug)]
pub struct CircuitBreaker<S> {
    inner: S,
}

#[pin_project]
#[derive(Debug)]
pub struct CircuitBreakerFuture<F> {
    #[pin]
    inner: F,
    #[pin]
    metrics: MetricsMap,
    #[pin]
    destination: String,
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

// === impl Metrics ===

struct DestinationMetrics {
    success_count: u32,
    failure_count: u32,
}

impl fmt::Debug for DestinationMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DestinationMetrics")
            .field("success_count", &self.success_count)
            .field("failure_count", &self.failure_count)
            .finish()
    }
}

// === impl Service ===

impl<S, A, B> tower::Service<http::Request<A>> for CircuitBreaker<S>
    where
        S: tower::Service<http::Request<A>, Response = http::Response<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    // type Future = S::Future;
    // type Future = future::MapOk<S::Future, fn(S::Response) -> S::Response>;
    // type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send + 'static>>;
    type Future = CircuitBreakerFuture<S::Future>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<A>) -> Self::Future {
        static mut METRICS: Option<HashMap<String, DestinationMetrics>> = None;

        let destination: String = req.headers().get("host").unwrap().to_str().unwrap().to_string();
        let first_key = "users:8080";

        unsafe {
            if METRICS.is_none() {
                METRICS = Some(HashMap::new());
            }

            let metrics = METRICS.as_mut().unwrap();

            metrics
                .entry(first_key.to_string())
                .or_insert(DestinationMetrics {
                    success_count: 0,
                    failure_count: 0,
                });

            info!("[MM]: {:?}", req.headers());
            info!("[MM]: {:?}", metrics.get(first_key));

            metrics
                .entry(destination.to_string())
                .or_insert(DestinationMetrics {
                    success_count: 0,
                    failure_count: 0,
                });
        }

        let future = self.inner.call(req);

        CircuitBreakerFuture {
            inner: future,
            metrics: unsafe { METRICS.as_mut().unwrap() },
            destination,
        }
    }
}

impl<F> Future for CircuitBreakerFuture<F>
    where
        F: TryFuture,
{
    type Output = Result<F::Ok, F::Error>;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let pinned_destination: Pin<&mut String> = this.destination;
        let pinned_metrics: Pin<&mut MetricsMap> = this.metrics;

        let destination: String = pinned_destination.to_string();
        let metrics: &mut MetricsMap = pinned_metrics.get_mut();

        match futures::ready!(this.inner.try_poll(cx)) {
            Ok(rsp) => {
                metrics
                    .entry(destination.to_string())
                    .or_insert(DestinationMetrics {
                        success_count: 0,
                        failure_count: 0,
                    });

                metrics.get_mut(&destination).unwrap().success_count += 1;

                Poll::Ready(Ok(rsp))
            },
            Err(err) => {
                metrics
                    .entry(destination.to_string())
                    .or_insert(DestinationMetrics {
                        success_count: 0,
                        failure_count: 0,
                    });

                metrics.get_mut(&destination).unwrap().failure_count += 1;

                Poll::Ready(Err(err))
            }
        }
    }
}