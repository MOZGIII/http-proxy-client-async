#![warn(missing_debug_implementations, rust_2018_idioms)]
#![feature(async_await)]

use futures::{executor, AsyncReadExt};
use http_proxy_client_async::*;
use merge_io::MergeIO;

#[test]
fn handshake_test() -> std::io::Result<()> {
    executor::block_on(async {
        let expected_req = "CONNECT 127.0.0.1:8080 HTTP/1.1\r\n\
                            Host: 127.0.0.1:8080\r\n\
                            \r\n";
        let sample_res = "HTTP/1.1 200 OK\r\n\
                          \r\n\
                          this is already the proxied content";

        let reader = std::io::Cursor::new(sample_res);
        let writer = std::io::Cursor::new(vec![0u8; 1024]);

        let socket = MergeIO::new(reader, writer);

        let mut tunnel_socket = handshake_and_wrap(socket, "127.0.0.1", 8080).await?;

        // Read all data from the socket.
        let mut data_at_tunnel = vec![];
        tunnel_socket.read_to_end(&mut data_at_tunnel).await?;

        // Validate that the data that arrived as part of the handshake is not
        // lost or corrupted.
        assert_eq!(
            data_at_tunnel,
            "this is already the proxied content".as_bytes()
        );

        // Deconstruct the tunnel into MergedIO socket and after-handshake-data
        // buffer.
        let (unwrapped_socket, data_after_handshake) = tunnel_socket.into_inner();

        // Ensure that at this point we don't have leftover data.
        assert_eq!(
            data_after_handshake, None,
            "we should've read everything that's left after the handshake"
        );

        // Deconstruct the unwrapped socket into cursor pairs.
        let (reader, writer) = unwrapped_socket.into_inner();

        // Esnure reader (the ones client read from) is at the end.
        assert_eq!(
            reader.position(),
            reader.get_ref().len() as u64,
            "readed was not read till the end"
        );

        // Ensure writer (the one client writes to) has the expected sample
        // data.
        assert_eq!(
            &writer.get_ref()[..writer.position() as usize],
            expected_req.as_bytes(),
            "writer didn't have the expected content"
        );

        Ok(())
    })
}
