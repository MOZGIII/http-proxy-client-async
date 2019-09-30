use futures::io::IoSlice;
use futures::prelude::*;
use std::io::Result;
use std::pin::Pin;
use std::task::{Context, Poll};

#[derive(Debug)]
pub struct PrependIoStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    read_prepend: Option<Vec<u8>>,
    wrapped: T,
}

impl<T> PrependIoStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    pub fn new(stream: T, read_prepend: Option<Vec<u8>>) -> Self {
        let read_prepend = match read_prepend {
            None => None,
            Some(ref boxed_buf) if boxed_buf.is_empty() => None,
            Some(boxed_buf) => Some(boxed_buf),
        };
        PrependIoStream {
            read_prepend,
            wrapped: stream,
        }
    }

    pub fn into_inner(self) -> (T, Option<Vec<u8>>) {
        (self.wrapped, self.read_prepend)
    }
}

impl<T> AsyncRead for PrependIoStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize>> {
        let self_mut = self.get_mut();
        if let Some(mut read_prepend) = self_mut.read_prepend.take() {
            let to_read = usize::min(buf.len(), read_prepend.len());
            let read_prepend_tail = read_prepend.split_off(to_read);

            if !read_prepend_tail.is_empty() {
                self_mut.read_prepend.replace(read_prepend_tail);
            }

            &mut buf[..to_read].copy_from_slice(read_prepend.as_ref());
            return Poll::Ready(Ok(to_read));
        }
        AsyncRead::poll_read(Pin::new(&mut self_mut.wrapped), cx, buf)
    }
}

impl<T> AsyncWrite for PrependIoStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize>> {
        AsyncWrite::poll_write(Pin::new(&mut self.get_mut().wrapped), cx, buf)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<Result<usize>> {
        AsyncWrite::poll_write_vectored(Pin::new(&mut self.get_mut().wrapped), cx, bufs)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.get_mut().wrapped), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        AsyncWrite::poll_close(Pin::new(&mut self.get_mut().wrapped), cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor;
    use merge_io::MergeIO;
    use std::io::Cursor;

    #[test]
    fn simple_prepended_read_test() -> Result<()> {
        executor::block_on(async {
            let reader = Cursor::new(vec![1, 2, 3, 4]);
            let writer = Cursor::new(vec![0u8; 1024]);
            let stream = MergeIO::new(reader, writer);

            let mut stream = PrependIoStream::new(stream, Some(vec![50, 60, 70, 80]));

            let mut buf = vec![];
            use futures::io::AsyncReadExt;
            stream.read_to_end(&mut buf).await?;

            assert_eq!(buf.as_slice(), &[50, 60, 70, 80, 1, 2, 3, 4]);

            Ok(())
        })
    }

    #[test]
    fn small_buffer_prepended_read_test() -> Result<()> {
        executor::block_on(async {
            let reader = Cursor::new(vec![1, 2, 3, 4]);
            let writer = Cursor::new(vec![0u8; 1024]);
            let stream = MergeIO::new(reader, writer);

            let mut stream = PrependIoStream::new(stream, Some(vec![50, 60, 70, 80]));

            // Expect to properly read prepend buf that's incomplete.
            let mut buf = [0u8; 2];
            let n = stream.read(&mut buf).await?;
            assert_eq!(n, 2);
            assert_eq!(&buf[..n], &[50, 60]);

            // Expect to properly read data up to the prepended buf end.
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).await?;
            assert_eq!(n, 2);
            assert_eq!(&buf[..n], &[70, 80]);

            // Expect to read data normally from the wrapped stream.
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).await?;
            assert_eq!(n, 4);
            assert_eq!(&buf[..n], &[1, 2, 3, 4]);

            Ok(())
        })
    }
}
