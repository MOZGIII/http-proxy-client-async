use futures::prelude::*;
use std::io::{Error, ErrorKind, Result};

pub async fn handshake<ARW>(stream: &mut ARW, host: &str, port: u16) -> Result<Vec<u8>>
where
    ARW: AsyncRead + AsyncWrite + Unpin,
{
    send_request(stream, host, port).await?;
    receive_response(stream).await
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

pub async fn receive_response<'buf, AR>(stream: &mut AR) -> Result<Vec<u8>>
where
    AR: AsyncRead + Unpin,
{
    let mut response_headers = [httparse::EMPTY_HEADER; 16];
    let mut buf = [0u8; 1024];
    let mut response = httparse::Response::new(&mut response_headers);

    // TODO: this implementaion is incorrect, as partial reads should not cause
    // errors, but rather trigger another read. It seems tricky (if at all
    // possible) with the current httparse API, so we're leaving it for now.
    // Most of the implementations using httparse suffer from the similar issue,
    // so we're not feeling too bad about it. And the documentation sucks ass -
    // all it sais here is we should read more data, it's not clear how are
    // supposed to pass that data to the parser to resume parsing where it
    // left off. There are no API usability tests or proper practial examples
    // at httparse either.
    let total = stream.read(&mut buf).await?;
    let status = response
        .parse(&buf[..total])
        .map_err(|err| Error::new(ErrorKind::InvalidData, err))?;
    match status {
        httparse::Status::Partial => {
            return Err(Error::new(ErrorKind::Other, "partial header read"))
        }
        httparse::Status::Complete(consumed) => return Ok(Vec::from(&buf[consumed..total])),
    };
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
            let data_after_handshake = receive_response(&mut socket).await?;
            assert_eq!(
                data_after_handshake.as_slice(),
                "this is already the proxied content".as_bytes()
            );
            Ok(())
        })
    }
}
