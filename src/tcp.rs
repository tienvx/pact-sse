use core::pin::Pin;
use core::task::{Context, Poll};
use futures::Stream;
use std::io;
use tokio::net::TcpListener;
use tokio::net::TcpStream;

pub struct TcpIncoming {
    pub inner: TcpListener,
}

impl Stream for TcpIncoming {
    type Item = Result<TcpStream, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner)
            .poll_accept(cx)
            .map_ok(|(stream, _)| stream)
            .map(Some)
    }
}
