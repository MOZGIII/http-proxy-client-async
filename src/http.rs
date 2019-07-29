use futures::prelude::*;
use std::io::{Error, ErrorKind, Result};

pub async fn handshake<ARW>(
    stream: &mut ARW,
    host: &str,
    port: u16,
    read_buf: &mut [u8],
) -> Result<Vec<u8>>
where
    ARW: AsyncRead + AsyncWrite + Unpin,
{
    send_request(stream, host, port).await?;
    receive_response(stream, read_buf).await
}

pub async fn send_request<AW>(stream: &mut AW, host: &str, port: u16) -> Result<()>
where
    AW: AsyncWrite + Unpin,
{
    let write_buf = format!(
        "CONNECT {0}:{1} HTTP/1.1\r\n\
         Host: {0}:{1}\r\n\
         \r\n",
        host, port,
    );
    use futures::AsyncWriteExt;
    stream.write_all(write_buf.as_bytes()).await
}

pub async fn receive_response<'buf, AR>(stream: &mut AR, read_buf: &mut [u8]) -> Result<Vec<u8>>
where
    AR: AsyncRead + Unpin,
{
    let mut response_headers = [httparse::EMPTY_HEADER; 16];

    // Happy path - we expect the response to be reasonably small and to come in
    // complete as a single buffer via a single read.
    // In this case we don't need to allocate and carry-on second buffer.

    let first_buf = {
        let total = stream.read(read_buf).await?;
        let buf = &read_buf[..total];
        let mut response = httparse::Response::new(&mut response_headers);

        let status = response
            .parse(buf)
            .map_err(|err| Error::new(ErrorKind::InvalidData, err))?;

        match status {
            httparse::Status::Partial => buf,
            httparse::Status::Complete(consumed) => return Ok(Vec::from(&buf[consumed..])),
        }
    };

    // We didn't exit early on error or completion, this means we're at slower
    // path and we need a carry-on buffer.

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
                return Ok(Vec::from(&carry_on_buf[consumed..]))
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor;
    use std::io::Cursor;

    #[test]
    fn send_request_test() -> Result<()> {
        executor::block_on(async {
            let sample_res = "CONNECT 127.0.0.1:8080 HTTP/1.1\r\n\
                              Host: 127.0.0.1:8080\r\n\
                              \r\n";
            let mut socket = Cursor::new(vec![0u8; 1024]);
            send_request(&mut socket, "127.0.0.1", 8080).await?;

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
            let data_after_handshake = receive_response(&mut socket, &mut read_buf).await?;
            assert_eq!(
                data_after_handshake.as_slice(),
                "this is already the proxied content".as_bytes()
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
            let data_after_handshake = receive_response(&mut socket, &mut read_buf).await?;

            // Prepare the estimates for the leftover data.
            let extra_read = (BUF_SIZE - (sample_handshake.len() % BUF_SIZE)) % BUF_SIZE;
            let expected_data = &sample_post_handshake_data[..extra_read];

            assert_eq!(data_after_handshake.as_slice(), expected_data.as_bytes());
            Ok(())
        })
    }
}
