use crate::{Outbound};
use futures::{TryFuture};
use pin_project::pin_project;
use linkerd_app_core::{svc::{self}, Error, io};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tracing::{info};

/// A strategy for circuit_breakering a NewService.
pub trait CircuitBreakerNewService<T> {
    type CircuitBreakerService;

    fn circuit_breaker(&self, target: &T) -> Self::CircuitBreakerService;
}

/// A strategy for circuit_breakering a Service.
pub trait CircuitBreakerService<Req> {
    type CircuitBreakerResponse;

    /// Monitors a response.
    fn circuit_breaker_request(&mut self, req: &Req) -> Self::CircuitBreakerResponse;
}

/// A strategy for circuit_breakering a Service's errors
pub trait CircuitBreakerError<E> {
    fn circuit_breaker_error(&mut self, err: &E);
}

#[derive(Clone, Debug)]
pub struct NewCircuitBreaker<N> {
    inner: N,
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
}

impl<N> NewCircuitBreaker<N> {
    pub fn layer() -> impl svc::layer::Layer<N, Service = Self> + Clone {
        svc::layer::mk(move |inner| Self {
            inner
        })
    }
}

// impl<S> Clone for CircuitBreaker<S>
// where
//     S: Clone,
// {
//     fn clone(&self) -> Self {
//         Self {
//             inner: self.inner.clone(),
//         }
//     }
// }

impl<T, N> svc::NewService<T> for NewCircuitBreaker<N>
    where
        N: svc::NewService<T>,
{
    type Service = CircuitBreaker<N::Service>;

    #[inline]
    fn new_service(&self, target: T) -> Self::Service {
        let inner = self.inner.new_service(target);
        CircuitBreaker { inner }
    }
}

// === impl CircuitBreaker ===

impl<S> CircuitBreaker<S> {
    #[allow(dead_code)]
    pub fn layer() -> impl svc::layer::Layer<S, Service = Self> + Clone {
        svc::layer::mk(move |inner| Self {
            inner,
        })
    }
}

impl<Req, S> svc::Service<Req> for CircuitBreaker<S>
    where
        S: svc::Service<Req>
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = CircuitBreakerFuture<S::Future>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match futures::ready!(self.inner.poll_ready(cx)) {
            Ok(()) => Poll::Ready(Ok(())),
            Err(err) => {
                Poll::Ready(Err(err))
            }
        }
    }

    #[inline]
    fn call(&mut self, req: Req) -> Self::Future {
        info!("[MM]: ");
        let inner = self.inner.call(req);
        CircuitBreakerFuture { inner }
    }
}

// === impl CircuitBreakerFuture ===

impl<F> Future for CircuitBreakerFuture<F>
    where
        F: TryFuture,
{
    type Output = Result<F::Ok, F::Error>;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match futures::ready!(this.inner.try_poll(cx)) {
            Ok(rsp) => Poll::Ready(Ok(rsp)),
            Err(err) => {
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<N> Outbound<N> {
    pub fn push_circuit_breaker<T, I, NSvc>(self) -> Outbound<
        svc::ArcNewService<
            T,
            impl svc::Service<I, Response = (), Error = Error, Future = impl Send>,
        >,
    >
    where
        I: io::AsyncRead + io::AsyncWrite + io::PeerAddr + std::fmt::Debug + Send + Unpin + 'static,
        N: svc::NewService<T, Service = NSvc> + Clone + Send + Sync + 'static,
        NSvc: svc::Service<I, Response = (), Error = Error> + Send + 'static,
        NSvc::Future: Send,
    {
        self.map_stack(|_, _, stack| {
            stack
                .push(NewCircuitBreaker::layer())
                .push(svc::ArcNewService::layer())
        })
    }
}




// #[derive(Clone, Debug, Default)]
// pub struct CheckRequest;
//
// impl<Req> CircuitBreakerService<Req> for CheckRequest {
//     type CircuitBreakerResponse = Self;
//
//     #[inline]
//     #[allow(dead_code)]
//     fn circuit_breaker_request(&mut self, _: &Req) -> Self::CircuitBreakerResponse {
//         info!("fuck off");
//         self.clone()
//     }
// }