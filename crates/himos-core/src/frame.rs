//! Wire format for the Segotep DM-series digital PSU serial protocol.
//!
//! A frame is `55 7E | LEN | CMD | PAYLOAD | CRC8 | AE` where `LEN` is
//! `1 (CMD) + payload.len() + 1 (CRC8)`, so the whole frame is `LEN + 4` bytes.
//!
//! The checksum is CRC-8 with polynomial `0x07` and initial value `0`, and the
//! two directions cover different ranges:
//!
//! * device to host: everything from `55 7E` through the end of the payload
//! * host to device: everything from `LEN` through the end of the payload
//!
//! Both rules were verified byte-for-byte against captured traffic.

use thiserror::Error;

/// Frame start marker.
pub const HEAD: [u8; 2] = [0x55, 0x7E];
/// Frame end marker.
pub const TAIL: u8 = 0xAE;
/// Largest payload the one-byte length field can describe.
pub const MAX_PAYLOAD: usize = 253;
/// Largest possible frame: [`MAX_PAYLOAD`] plus length, command, checksum, and markers.
pub const MAX_FRAME: usize = MAX_PAYLOAD + 6;
/// Frame length overhead: two header bytes, the length byte, and the tail.
const FRAME_OVERHEAD: usize = 4;
/// Bytes buffered before the splitter declares the stream unsynchronised.
const MAX_BUFFER: usize = MAX_FRAME * 4;

/// CRC-8 (poly `0x07`, init `0`) used by both directions.
#[must_use]
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for byte in data {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Reasons a frame cannot be built or parsed.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FrameError {
    /// The payload is larger than the one-byte length field can describe.
    #[error("payload of {0} bytes exceeds the {MAX_PAYLOAD}-byte limit")]
    PayloadTooLong(usize),
    /// The slice is shorter than the smallest possible frame.
    #[error("frame too short: {0} bytes")]
    TooShort(usize),
    /// The header or tail marker is missing.
    #[error("bad frame markers")]
    BadMarkers,
    /// The declared length does not match the slice length.
    #[error("declared length {declared} does not match {actual} bytes")]
    BadLength {
        /// Value of the length byte.
        declared: u8,
        /// Actual frame length.
        actual: usize,
    },
    /// The checksum does not match.
    #[error("bad checksum: expected {expected:#04x}, got {got:#04x}")]
    BadCrc {
        /// Checksum computed from the frame body.
        expected: u8,
        /// Checksum carried in the frame.
        got: u8,
    },
}

/// One protocol frame: a command byte and its payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    cmd: u8,
    payload: Vec<u8>,
}

impl Frame {
    /// Builds a frame from a command byte and payload.
    ///
    /// Payload length is validated by [`Frame::encode`], which is the point
    /// where the one-byte length field must fit.
    #[must_use]
    pub fn new(cmd: u8, payload: impl Into<Vec<u8>>) -> Self {
        Self {
            cmd,
            payload: payload.into(),
        }
    }

    /// Command byte (`0x02` status, `0x03` model, ...).
    #[must_use]
    pub fn cmd(&self) -> u8 {
        self.cmd
    }

    /// Payload bytes, without command or checksum.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Encodes this frame with the host-to-device checksum rule.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError::PayloadTooLong`] when the payload exceeds
    /// [`MAX_PAYLOAD`] bytes; the protocol has no way to express it.
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        let len = self.payload.len() + 2;
        let len_byte =
            u8::try_from(len).map_err(|_| FrameError::PayloadTooLong(self.payload.len()))?;

        let mut body = Vec::with_capacity(len);
        body.push(len_byte);
        body.push(self.cmd);
        body.extend_from_slice(&self.payload);

        let mut out = Vec::with_capacity(len + FRAME_OVERHEAD - 1);
        out.extend_from_slice(&HEAD);
        out.extend_from_slice(&body);
        out.push(crc8(&body));
        out.push(TAIL);
        Ok(out)
    }

    /// Parses a device-to-host frame, validating markers, length, and checksum.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError`] when the slice is too short, has wrong markers,
    /// declares a different length, or fails the checksum.
    pub fn decode(raw: &[u8]) -> Result<Self, FrameError> {
        if raw.len() < FRAME_OVERHEAD + 2 {
            return Err(FrameError::TooShort(raw.len()));
        }
        if raw[..2] != HEAD || raw[raw.len() - 1] != TAIL {
            return Err(FrameError::BadMarkers);
        }
        let declared = raw[2];
        if usize::from(declared) + FRAME_OVERHEAD != raw.len() {
            return Err(FrameError::BadLength {
                declared,
                actual: raw.len(),
            });
        }
        let expected = crc8(&raw[..raw.len() - 2]);
        let got = raw[raw.len() - 2];
        if expected != got {
            return Err(FrameError::BadCrc { expected, got });
        }
        Ok(Self {
            cmd: raw[3],
            payload: raw[4..raw.len() - 2].to_vec(),
        })
    }
}

/// Incremental splitter that turns a serial byte stream into frames.
#[derive(Debug, Default)]
pub struct FrameReader {
    buffer: Vec<u8>,
}

impl FrameReader {
    /// Creates an empty reader.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends bytes and returns every complete, valid frame now available.
    ///
    /// Invalid candidates are skipped to the next header, so noise between
    /// frames does not desynchronise the reader. A buffer that grows past four
    /// times the maximum frame length without producing a frame is discarded.
    pub fn feed(&mut self, data: &[u8]) -> Vec<Frame> {
        self.buffer.extend_from_slice(data);
        let mut frames = Vec::new();

        loop {
            let Some(start) = self.buffer.windows(2).position(|w| w == HEAD) else {
                // Keep a trailing 0x55: it may be the first half of a header.
                let keep = usize::from(self.buffer.last() == Some(&HEAD[0]));
                let drop = self.buffer.len() - keep;
                self.buffer.drain(..drop);
                break;
            };
            if start > 0 {
                self.buffer.drain(..start);
            }
            if self.buffer.len() < 3 {
                break;
            }
            let total = usize::from(self.buffer[2]) + FRAME_OVERHEAD;
            if self.buffer.len() < total {
                break;
            }
            match Frame::decode(&self.buffer[..total]) {
                Ok(frame) => {
                    frames.push(frame);
                    self.buffer.drain(..total);
                }
                Err(_) => {
                    // Skip this header, not a single byte, so a run of noise
                    // cannot make the scan quadratic.
                    self.buffer.drain(..HEAD.len());
                }
            }
        }

        if self.buffer.len() > MAX_BUFFER {
            self.buffer.clear();
        }
        frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_length_one_and_recovers_the_next_frame() {
        let malformed = [0x55, 0x7e, 0x01, 0x97, 0xae];
        assert_eq!(Frame::decode(&malformed), Err(FrameError::TooShort(5)));
        let valid = hex_to_bytes("557e0484000051ae");
        let mut reader = FrameReader::new();
        assert_eq!(reader.feed(&malformed), Vec::<Frame>::new());
        assert_eq!(reader.feed(&valid).len(), 1);
    }

    #[test]
    fn maximum_payload_fits_the_documented_frame_size() {
        let mut raw = Frame::new(0x02, vec![0; MAX_PAYLOAD])
            .encode()
            .expect("maximum payload fits");
        assert_eq!(raw.len(), MAX_FRAME);
        raw[MAX_FRAME - 2] = crc8(&raw[..MAX_FRAME - 2]);
        assert_eq!(
            Frame::decode(&raw)
                .expect("valid device frame")
                .payload()
                .len(),
            MAX_PAYLOAD
        );
    }

    #[test]
    fn host_frames_match_captured_bytes() {
        // Captured from HiMOS 1.6.1 talking to a DM-850G.
        let cases: &[(u8, &[u8], &str)] = &[
            (0x04, &[0x00, 0x00], "557e04040000f3ae"),
            (0x13, &[0x00, 0x01], "557e0413000140ae"),
            (0x1e, &[0x00, 0x00], "557e041e0000d6ae"),
        ];
        for (cmd, payload, expected) in cases {
            let got = Frame::new(*cmd, *payload)
                .encode()
                .expect("payload fits the protocol");
            assert_eq!(hex(&got), *expected, "cmd {cmd:#04x}");
        }
    }

    #[test]
    fn oversized_payload_is_rejected() {
        let frame = Frame::new(0x04, vec![0u8; MAX_PAYLOAD + 1]);
        assert_eq!(
            frame.encode(),
            Err(FrameError::PayloadTooLong(MAX_PAYLOAD + 1))
        );
    }

    #[test]
    fn decodes_captured_status_frame() {
        let raw =
            hex_to_bytes("557e1e02f10caf13392fc7130000150bd20bf9010d349d43b121cc0800000000d9ae");
        let frame = Frame::decode(&raw).expect("captured frame decodes");
        assert_eq!(frame.cmd(), 0x02);
        assert_eq!(frame.payload().len(), 28);
    }

    #[test]
    fn rejects_bad_checksum() {
        let mut raw = hex_to_bytes("557e04040000f3ae");
        let last = raw.len() - 2;
        raw[last] ^= 0xFF;
        assert!(matches!(
            Frame::decode(&raw),
            Err(FrameError::BadCrc { .. })
        ));
    }

    #[test]
    fn reader_handles_split_and_noisy_stream() {
        // Captured device frames: a status frame followed by the serial number.
        let stream = hex_to_bytes(
            "00ff\
             557e1e02f10caf13392fc7130000150bd20bf9010d349d43b121cc0800000000d9ae\
             557e12055342525936423530363030363133352049ae",
        );
        let mut reader = FrameReader::new();
        let mut frames = Vec::new();
        for chunk in stream.chunks(3) {
            frames.extend(reader.feed(chunk));
        }
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].cmd(), 0x02);
        assert_eq!(frames[1].cmd(), 0x05);
    }

    #[test]
    fn reader_skips_a_false_header_without_losing_the_next_frame() {
        // 55 7e claims 0x1e bytes but the payload is noise, then a real frame follows.
        let stream = hex_to_bytes(
            "557e1e00\
             557e1e02f10caf13392fc7130000150bd20bf9010d349d43b121cc0800000000d9ae",
        );
        let mut reader = FrameReader::new();
        let frames = reader.feed(&stream);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].cmd(), 0x02);
    }

    #[test]
    fn reader_discards_an_unbounded_noise_run() {
        let mut reader = FrameReader::new();
        let noise = vec![0x7e_u8; MAX_BUFFER + 16];
        let frames = reader.feed(&noise);
        assert_eq!(frames.len(), 0, "noise must not produce frames");
        // The splitter keeps only a handful of bytes once noise passes the cap.
        assert!(
            reader.buffer.len() < MAX_FRAME,
            "noise buffer must be discarded"
        );
    }

    fn hex(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            let _ = write!(out, "{byte:02x}");
        }
        out
    }

    fn hex_to_bytes(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
            .collect()
    }
}
