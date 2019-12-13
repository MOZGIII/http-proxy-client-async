use futures_io::{AsyncRead, AsyncWrite, IoSlice, IoSliceMut};
use futures_util::io::{AsyncReadExt, Chain, Cursor};
use std::io::Result;
use std::pin::Pin;
use std::task::{Context, Poll};

#[derive(Debug)]
pub enum PrependIoStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    Chain(Chain<Cursor<Vec<u8>>, T>),
    Plain(T),
}

impl<T> PrependIoStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    pub fn from_vec(stream: T, read_prepend: Option<Vec<u8>>) -> Self {
        let read_prepend = match read_prepend {
            None => None,
            Some(ref boxed_buf) if boxed_buf.is_empty() => None,
            Some(boxed_buf) => Some(boxed_buf),
        };
        match read_prepend {
            Some(read_prepend) => Self::from_cursor(stream, Cursor::new(read_prepend)),
            None => Self::plain(stream),
        }
    }

    pub fn from_cursor(stream: T, read_prepend: Cursor<Vec<u8>>) -> Self {
        Self::chain(read_prepend.chain(stream))
    }

    pub fn chain(chain: Chain<Cursor<Vec<u8>>, T>) -> Self {
        PrependIoStream::Chain(chain)
    }

    pub fn plain(stream: T) -> Self {
        PrependIoStream::Plain(stream)
    }

    pub fn into_inner(self) -> (T, Option<Cursor<Vec<u8>>>) {
        match self {
            PrependIoStream::Chain(chain) => {
                let (cursor, stream) = chain.into_inner();
                (stream, Some(cursor))
            }
            PrependIoStream::Plain(stream) => (stream, None),
        }
    }

    pub fn pending_prepend_data(&self) -> &[u8] {
        match self {
            PrependIoStream::Chain(chain) => {
                let (cursor, _) = chain.get_ref();
                let pos = cursor.position() as usize;
                let vec = cursor.get_ref();
                &vec[pos..]
            }
            PrependIoStream::Plain(_) => &[],
        }
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
        match self.get_mut() {
            PrependIoStream::Plain(ref mut stream) => {
                AsyncRead::poll_read(Pin::new(stream), cx, buf)
            }
            PrependIoStream::Chain(ref mut chain) => AsyncRead::poll_read(Pin::new(chain), cx, buf),
        }
    }

    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<Result<usize>> {
        match self.get_mut() {
            PrependIoStream::Plain(ref mut stream) => {
                AsyncRead::poll_read_vectored(Pin::new(stream), cx, bufs)
            }
            PrependIoStream::Chain(ref mut chain) => {
                AsyncRead::poll_read_vectored(Pin::new(chain), cx, bufs)
            }
        }
    }
}

impl<T> AsyncWrite for PrependIoStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize>> {
        match self.get_mut() {
            PrependIoStream::Plain(ref mut stream) => {
                AsyncWrite::poll_write(Pin::new(stream), cx, buf)
            }
            PrependIoStream::Chain(chain) => {
                let (_, stream) = chain.get_mut();
                AsyncWrite::poll_write(Pin::new(stream), cx, buf)
            }
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<Result<usize>> {
        match self.get_mut() {
            PrependIoStream::Plain(ref mut stream) => {
                AsyncWrite::poll_write_vectored(Pin::new(stream), cx, bufs)
            }
            PrependIoStream::Chain(chain) => {
                let (_, stream) = chain.get_mut();
                AsyncWrite::poll_write_vectored(Pin::new(stream), cx, bufs)
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        match self.get_mut() {
            PrependIoStream::Plain(ref mut stream) => AsyncWrite::poll_flush(Pin::new(stream), cx),
            PrependIoStream::Chain(chain) => {
                let (_, stream) = chain.get_mut();
                AsyncWrite::poll_flush(Pin::new(stream), cx)
            }
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        match self.get_mut() {
            PrependIoStream::Plain(ref mut stream) => AsyncWrite::poll_close(Pin::new(stream), cx),
            PrependIoStream::Chain(chain) => {
                let (_, stream) = chain.get_mut();
                AsyncWrite::poll_close(Pin::new(stream), cx)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor;
    use merge_io::MergeIO;

    #[test]
    fn simple_prepended_read_test() -> Result<()> {
        executor::block_on(async {
            let reader = Cursor::new(vec![1, 2, 3, 4]);
            let writer = Cursor::new(vec![0u8; 1024]);
            let stream = MergeIO::new(reader, writer);

            let mut stream = PrependIoStream::from_vec(stream, Some(vec![50, 60, 70, 80]));

            let mut buf = vec![];
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

            let mut stream = PrependIoStream::from_vec(stream, Some(vec![50, 60, 70, 80]));

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
