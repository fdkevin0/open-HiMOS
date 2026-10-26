//! Session helpers that pair a byte link with the frame splitter.

use std::io;

use crate::frame::{Frame, FrameReader};

/// A bidirectional byte link to the PSU (serial port, mock, or test double).
pub trait Link {
    /// Reads up to `buf.len()` bytes, returning how many were read.
    ///
    /// # Errors
    ///
    /// Propagates transport failures.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;

    /// Writes every byte of `data`.
    ///
    /// # Errors
    ///
    /// Propagates transport failures.
    fn write_all(&mut self, data: &[u8]) -> io::Result<()>;
}

/// A link plus the incremental frame splitter.
#[derive(Debug)]
pub struct Session<L> {
    link: L,
    reader: FrameReader,
}

impl<L: Link> Session<L> {
    /// Wraps a link.
    pub fn new(link: L) -> Self {
        Self {
            link,
            reader: FrameReader::new(),
        }
    }

    /// Sends one frame to the device.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::InvalidInput`] when the frame cannot be encoded
    /// (payload longer than the protocol allows), or the transport error.
    pub fn send(&mut self, frame: &Frame) -> io::Result<()> {
        let bytes = frame
            .encode()
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
        self.link.write_all(&bytes)
    }

    /// Reads whatever is available and returns the frames completed by it.
    ///
    /// # Errors
    ///
    /// Propagates transport failures; a timeout with zero bytes is not an error.
    pub fn poll(&mut self, buf: &mut [u8]) -> io::Result<Vec<Frame>> {
        let n = self.link.read(buf)?;
        if n == 0 {
            return Ok(Vec::new());
        }
        Ok(self.reader.feed(&buf[..n]))
    }

    /// Consumes the session and returns the underlying link.
    pub fn into_inner(self) -> L {
        self.link
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dm850g::connect;

    struct MockLink {
        read: Vec<u8>,
        writes: Vec<u8>,
    }

    impl Link for MockLink {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(self.read.len());
            buf[..n].copy_from_slice(&self.read[..n]);
            self.read.drain(..n);
            Ok(n)
        }

        fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
            self.writes.extend_from_slice(data);
            Ok(())
        }
    }

    #[test]
    fn session_sends_and_reassembles() {
        let link = MockLink {
            read: vec![0x55, 0x7e, 0x04, 0x84, 0x00, 0x00, 0x51, 0xae],
            writes: Vec::new(),
        };
        let mut session = Session::new(link);
        session.send(&connect()).expect("write succeeds");
        let mut buf = [0u8; 64];
        let frames = session.poll(&mut buf).expect("read succeeds");
        assert_eq!(frames.len(), 1);
        assert_eq!(
            session.into_inner().writes,
            connect().encode().expect("connect frame encodes")
        );
    }
}
