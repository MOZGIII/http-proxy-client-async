#![warn(missing_debug_implementations, rust_2018_idioms)]
#![feature(async_await)]

pub mod flow;
pub mod http;
pub mod prepend_io_stream;

use futures::prelude::*;
use prepend_io_stream::PrependIoStream;
use std::io::Result;

pub use crate::http::*;
pub use flow::{HandshakeOutcome, ResponseParts};

pub async fn handshake_and_wrap<ARW>(
    mut stream: ARW,
    host: &str,
    port: u16,
    request_headers: &HeaderMap,
    read_buf: &mut [u8],
) -> Result<Outcome<PrependIoStream<ARW>>>
where
    ARW: AsyncRead + AsyncWrite + Unpin,
{
    let HandshakeOutcome {
        response_parts,
        data_after_handshake,
    } = flow::handshake(&mut stream, host, port, request_headers, read_buf).await?;

    Ok(Outcome {
        response_parts,
        stream: PrependIoStream::new(stream, Some(data_after_handshake.into())),
    })
}

#[derive(Debug)]
pub struct Outcome<T> {
    pub response_parts: ResponseParts,
    pub stream: T,
}
