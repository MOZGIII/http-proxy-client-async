use futures::prelude::*;
use std::io::{Error, ErrorKind, Result};

use crate::http::HeaderMap;

mod handshake_outcome;
mod request;

pub use handshake_outcome::{HandshakeOutcome, ResponseParts};

pub async fn handshake<ARW>(
    stream: &mut ARW,
    host: &str,
    port: u16,
    request_headers: &HeaderMap,
    read_buf: &mut [u8],
) -> Result<HandshakeOutcome>
where
    ARW: AsyncRead + AsyncWrite + Unpin,
{
    send_request(stream, host, port, request_headers).await?;
    receive_response(stream, read_buf).await
}

pub async fn send_request<AW>(
    stream: &mut AW,
    host: &str,
    port: u16,
    headers: &HeaderMap,
) -> Result<()>
where
    AW: AsyncWrite + Unpin,
{
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    request::write(&mut buf, host, port, headers)?;

    use futures::AsyncWriteExt;
    stream.write_all(buf.as_slice()).await
}

pub async fn receive_response<'buf, AR>(
    stream: &mut AR,
    read_buf: &mut [u8],
) -> Result<HandshakeOutcome>
where
    AR: AsyncRead + Unpin,
{
    // Happy path - we expect the response to be reasonably small and to come in
    // complete as a single buffer via a single read.
    // In this case we don't need to allocate and carry-on second buffer.

    let first_buf = {
        let total = stream.read(read_buf).await?;
        let buf = &read_buf[..total];

        let mut response_headers = [httparse::EMPTY_HEADER; 16];
        let mut response = httparse::Response::new(&mut response_headers);

        let status = response
            .parse(buf)
            .map_err(|err| Error::new(ErrorKind::InvalidData, err))?;

        match status {
            httparse::Status::Partial => buf,
            httparse::Status::Complete(consumed) => {
                return Ok(HandshakeOutcome::new(response, Vec::from(&buf[consumed..])))
            }
        }
    };

    // We didn't exit early on error or completion, this means we're at slower
    // path and we need a carry-on buffer.

    // TODO: allow user to customize the data structure used for a carry-on
    // buffer. This is useful in case user wants to limit the amount of memory
    // this buffer can grow to, or for the cases when a more optimized data
    // structure is at hand.
    let mut carry_on_buf = Vec::from(first_buf);
    loop {
        let total = stream.read(read_buf).await?;
        let buf = &read_buf[..total];
        carry_on_buf.extend_from_slice(buf);

        let mut response_headers = [httparse::EMPTY_HEADER; 16];
        let mut response = httparse::Response::new(&mut response_headers);

        let status = response
            .parse(carry_on_buf.as_slice())
            .map_err(|err| Error::new(ErrorKind::InvalidData, err))?;
        match status {
            httparse::Status::Partial => continue,
            httparse::Status::Complete(consumed) => {
                return Ok(HandshakeOutcome::new(
                    response,
                    Vec::from(&carry_on_buf[consumed..]),
                ))
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::HeaderValue;
    use futures::executor;
    use std::io::Cursor;

    #[test]
    fn send_request_without_headers() -> Result<()> {
        executor::block_on(async {
            let sample_res = "CONNECT 127.0.0.1:8080 HTTP/1.1\r\n\
                              Host: 127.0.0.1:8080\r\n\
                              \r\n";
            let mut socket = Cursor::new(vec![0u8; 1024]);
            let headers = HeaderMap::new();
            send_request(&mut socket, "127.0.0.1", 8080, &headers).await?;

            assert_eq!(
                &socket.get_ref()[..socket.position() as usize],
                sample_res.as_bytes(),
            );
            Ok(())
        })
    }

    #[test]
    fn send_request_with_headers() -> Result<()> {
        executor::block_on(async {
            let sample_res = "CONNECT 127.0.0.1:8080 HTTP/1.1\r\n\
                              Host: 127.0.0.1:8080\r\n\
                              proxy-authorization: Basic aGVsbG86d29ybGQ=\r\n\
                              \r\n";
            let mut socket = Cursor::new(vec![0u8; 1024]);
            let mut headers = HeaderMap::new();
            headers.insert(
                "Proxy-Authorization",
                HeaderValue::from_static("Basic aGVsbG86d29ybGQ="),
            );
            send_request(&mut socket, "127.0.0.1", 8080, &headers).await?;

            assert_eq!(
                &socket.get_ref()[..socket.position() as usize],
                sample_res.as_bytes(),
            );
            Ok(())
        })
    }

    #[test]
    fn receive_response_test() -> Result<()> {
        executor::block_on(async {
            let sample_res = "HTTP/1.1 200 OK\r\n\
                              \r\n\
                              this is already the proxied content";
            let mut socket = Cursor::new(sample_res);
            let mut read_buf = [0u8; 1024];
            let outcome = receive_response(&mut socket, &mut read_buf).await?;
            assert_eq!(
                outcome.data_after_handshake.as_slice(),
                "this is already the proxied content".as_bytes()
            );
            assert_eq!(outcome.response_parts.status_code, 200);
            assert_eq!(outcome.response_parts.reason_phrase, "OK");
            assert_eq!(outcome.response_parts.headers.len(), 0);
            Ok(())
        })
    }

    #[test]
    fn receive_response_with_headers() -> Result<()> {
        executor::block_on(async {
            let sample_res = "HTTP/1.1 200 OK\r\n\
                              X-Custom: Sample Value\r\n\
                              \r\n\
                              this is already the proxied content";
            let mut socket = Cursor::new(sample_res);
            let mut read_buf = [0u8; 1024];
            let outcome = receive_response(&mut socket, &mut read_buf).await?;
            assert_eq!(
                outcome.data_after_handshake.as_slice(),
                "this is already the proxied content".as_bytes()
            );
            assert_eq!(outcome.response_parts.status_code, 200);
            assert_eq!(outcome.response_parts.reason_phrase, "OK");
            assert_eq!(outcome.response_parts.headers.len(), 1);
            assert_eq!(
                outcome.response_parts.headers.get("x-custom").unwrap(),
                &"Sample Value"
            );
            Ok(())
        })
    }

    #[test]
    fn receive_response_small_read_buf_test() -> Result<()> {
        executor::block_on(async {
            let sample_handshake = "HTTP/1.1 200 OK\r\n\
                                    \r\n";
            let sample_post_handshake_data = "this is already the proxied content";
            let sample_res = sample_handshake.to_string() + sample_post_handshake_data;
            let mut socket = Cursor::new(sample_res);

            // Use small read buffer size to force non-happy-path.
            const BUF_SIZE: usize = 4;
            let mut read_buf = [0u8; BUF_SIZE];
            let outcome = receive_response(&mut socket, &mut read_buf).await?;

            // Prepare the estimates for the leftover data.
            let extra_read = (BUF_SIZE - (sample_handshake.len() % BUF_SIZE)) % BUF_SIZE;
            let expected_data = &sample_post_handshake_data[..extra_read];

            assert_eq!(
                outcome.data_after_handshake.as_slice(),
                expected_data.as_bytes()
            );
            assert_eq!(outcome.response_parts.status_code, 200);
            assert_eq!(outcome.response_parts.reason_phrase, "OK");
            assert_eq!(outcome.response_parts.headers.len(), 0);
            Ok(())
        })
    }
}
