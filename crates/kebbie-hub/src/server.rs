use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::Router;
use axum::extract::ConnectInfo;
use bytes::Bytes;
use http_body::{Body, Frame, SizeHint};
use hyper::body::Incoming;
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};
use tokio::time::{Instant, Sleep};

use crate::{Hub, SWEEP_EVERY};

const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

type BodyError = Box<dyn std::error::Error + Send + Sync>;

struct TimeoutBody {
    inner: Incoming,
    timeout: Duration,
    deadline: Pin<Box<Sleep>>,
}

impl TimeoutBody {
    fn new(inner: Incoming, timeout: Duration) -> TimeoutBody {
        TimeoutBody {
            inner,
            timeout,
            deadline: Box::pin(tokio::time::sleep(timeout)),
        }
    }
}

impl Body for TimeoutBody {
    type Data = Bytes;
    type Error = BodyError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BodyError>>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(frame) => {
                this.deadline.as_mut().reset(Instant::now() + this.timeout);
                Poll::Ready(frame.map(|frame| frame.map_err(BodyError::from)))
            }
            Poll::Pending => match this.deadline.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(Some(Err("the request body stalled".into()))),
                Poll::Pending => Poll::Pending,
            },
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

pub async fn serve(
    listener: TcpListener,
    hub: Arc<Hub>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let sweeper = Arc::clone(&hub);
    let sweeping = tokio::spawn(async move {
        let mut ticks = tokio::time::interval(SWEEP_EVERY);
        loop {
            ticks.tick().await;
            if let Err(error) = sweeper.sweep() {
                eprintln!("kebbie-hub: sweep failed: {error}");
            }
        }
    });
    let app = crate::router(Arc::clone(&hub));
    let slots = Arc::new(Semaphore::new(hub.config().max_connections.max(1)));
    let (stop, stopping) = watch::channel(false);
    let (open, all_closed) = watch::channel(());
    let mut shutdown = std::pin::pin!(shutdown);
    loop {
        let slot = tokio::select! {
            slot = Arc::clone(&slots).acquire_owned() => slot.expect("the semaphore is never closed"),
            () = &mut shutdown => break,
        };
        let (stream, peer) = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(accepted) => accepted,
                Err(error) => {
                    eprintln!("kebbie-hub: accept failed: {error}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            },
            () = &mut shutdown => break,
        };
        tokio::spawn(connection(
            stream,
            peer,
            app.clone(),
            Arc::clone(&hub),
            stopping.clone(),
            (all_closed.clone(), slot),
        ));
    }
    hub.closing.send_replace(true);
    let _ = stop.send(true);
    drop(all_closed);
    let _ = tokio::time::timeout(SHUTDOWN_GRACE, open.closed()).await;
    sweeping.abort();
    Ok(())
}

async fn connection(
    stream: TcpStream,
    peer: SocketAddr,
    app: Router,
    hub: Arc<Hub>,
    mut stopping: watch::Receiver<bool>,
    _held: (watch::Receiver<()>, OwnedSemaphorePermit),
) {
    let body_timeout = hub.config().body_frame_timeout;
    let service = hyper::service::service_fn(move |request: hyper::Request<Incoming>| {
        let mut request =
            request.map(|body| axum::body::Body::new(TimeoutBody::new(body, body_timeout)));
        request.extensions_mut().insert(ConnectInfo(peer));
        let mut app = app.clone();
        async move { tower_service::Service::call(&mut app, request).await }
    });
    let mut builder = hyper::server::conn::http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(hub.config().header_read_timeout);
    let connection = builder.serve_connection(TokioIo::new(stream), service);
    let mut connection = std::pin::pin!(connection);
    let stop = async {
        let _ = stopping.wait_for(|stop| *stop).await;
    };
    tokio::select! {
        _ = connection.as_mut() => {}
        () = stop => {
            connection.as_mut().graceful_shutdown();
            let _ = connection.await;
        }
    }
}
