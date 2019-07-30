#![warn(missing_debug_implementations, rust_2018_idioms)]
#![feature(async_await)]

pub mod flow;
pub mod prepend_io_stream;

use futures::prelude::*;
use prepend_io_stream::PrependIoStream;
use std::io::Result;

pub async fn handshake_and_wrap<ARW>(
    mut stream: ARW,
    host: &str,
    port: u16,
    read_buf: &mut [u8],
) -> Result<PrependIoStream<ARW>>
where
    ARW: AsyncRead + AsyncWrite + Unpin,
{
    let data_after_handshake = flow::handshake(&mut stream, host, port, read_buf).await?;
    Ok(PrependIoStream::new(
        stream,
        Some(data_after_handshake.into()),
    ))
}
