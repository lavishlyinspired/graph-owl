//! Bolt's chunked transfer framing — the layer between PackStream's byte
//! stream and the raw socket.
//!
//! Every Bolt message is split into one or more chunks, each a two-byte
//! big-endian length header followed by that many bytes of message data, and
//! terminated by a zero-length chunk (`00 00`). This exists independently of
//! PackStream so that a receiver can find message boundaries — "come back
//! with more bytes" and "this message is complete" — without parsing the
//! PackStream content itself.
//!
//! Kept pure and separate from the socket it eventually reads from, the same
//! way [`crate::packstream::decode`] is pure and separate from the chunking
//! that carries it — both need the identical "not yet, not an error, and a
//! declared length is checked before anything is allocated from it"
//! property, and both get it by taking bytes in and handing bytes/values
//! back rather than owning I/O.

/// The largest chunk a two-byte length header can express.
const MAX_CHUNK_SIZE: usize = 0xFFFF;

/// Split one message into Bolt's chunked wire format, terminated by the
/// zero-length end marker.
///
/// A message under [`MAX_CHUNK_SIZE`] becomes exactly one chunk; a larger one
/// splits across as many as it needs. An empty message still produces a
/// single `00 00` — the terminator alone is a valid (empty) message on the
/// wire, distinct from sending nothing at all.
#[must_use]
pub fn encode(message: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(message.len() + 2);
    // `[].chunks(n)` yields no pieces at all, so an empty message correctly
    // produces just the terminator below — a bare `00 00` — with no empty
    // data chunk ahead of it.
    for piece in message.chunks(MAX_CHUNK_SIZE) {
        push_chunk(&mut out, piece);
    }
    out.extend_from_slice(&[0x00, 0x00]);
    out
}

fn push_chunk(out: &mut Vec<u8>, piece: &[u8]) {
    #[allow(clippy::cast_possible_truncation)] // piece.len() <= MAX_CHUNK_SIZE by construction
    out.extend_from_slice(&(piece.len() as u16).to_be_bytes());
    out.extend_from_slice(piece);
}

/// Why bytes fed to a [`Decoder`] could not be assembled into a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ChunkError {
    /// The message assembled so far, plus the newly declared chunk, would
    /// exceed the byte budget — refused **before** the chunk's data is
    /// appended, the same allocation-guard property PackStream's decoder
    /// holds for a single length-prefixed value.
    #[error("chunked message exceeds the {limit}-byte budget")]
    MessageTooLarge { limit: usize },
}

/// Accumulates bytes read off a socket and yields complete messages.
///
/// A connection reads whatever the kernel hands back on one read call, which
/// rarely lines up with chunk or message boundaries — so bytes are fed in as
/// they arrive and messages come out only once their terminator has been
/// seen. This mirrors [`crate::packstream::decode`]'s truncation contract at
/// the framing layer: incomplete input is a normal, recoverable state, not
/// an error.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
    message: Vec<u8>,
}

impl Decoder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append bytes just read from the socket.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Pull as many complete messages as the fed bytes currently contain.
    ///
    /// Returns `Ok(None)` once no complete chunk remains in the buffer — the
    /// normal "read more from the socket" outcome. A message spanning
    /// several `feed` calls stays assembled in `self.message` between them.
    ///
    /// # Errors
    ///
    /// [`ChunkError::MessageTooLarge`] once the in-progress message would
    /// exceed `max_message_bytes`, checked before the offending chunk's
    /// bytes are appended to it.
    pub fn next_message(
        &mut self,
        max_message_bytes: usize,
    ) -> Result<Option<Vec<u8>>, ChunkError> {
        loop {
            if self.buf.len() < 2 {
                return Ok(None);
            }
            #[allow(clippy::cast_lossless)]
            let len = u16::from_be_bytes([self.buf[0], self.buf[1]]) as usize;
            if self.buf.len() < 2 + len {
                return Ok(None);
            }

            if len == 0 {
                // The terminator. Whatever accumulated in `self.message` —
                // possibly nothing, for an intentionally empty message — is
                // the complete one.
                let complete = std::mem::take(&mut self.message);
                self.buf.drain(..2);
                return Ok(Some(complete));
            }

            if self.message.len() + len > max_message_bytes {
                return Err(ChunkError::MessageTooLarge {
                    limit: max_message_bytes,
                });
            }
            self.message.extend_from_slice(&self.buf[2..2 + len]);
            self.buf.drain(..2 + len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_under_the_chunk_limit_becomes_one_chunk_plus_terminator() {
        let encoded = encode(&[0xAA, 0xBB, 0xCC]);
        assert_eq!(encoded, vec![0x00, 0x03, 0xAA, 0xBB, 0xCC, 0x00, 0x00]);
    }

    #[test]
    fn an_empty_message_is_a_bare_terminator() {
        assert_eq!(encode(&[]), vec![0x00, 0x00]);
    }

    #[test]
    fn a_message_over_the_chunk_limit_splits_across_chunks() {
        let message = vec![0x7Au8; MAX_CHUNK_SIZE + 10];
        let encoded = encode(&message);
        assert_eq!(
            &encoded[0..2],
            &[0xFF, 0xFF],
            "first chunk header is the max size"
        );
        assert_eq!(&encoded[2..2 + MAX_CHUNK_SIZE], &message[..MAX_CHUNK_SIZE]);
        let second_header_at = 2 + MAX_CHUNK_SIZE;
        assert_eq!(
            &encoded[second_header_at..second_header_at + 2],
            &[0x00, 0x0A]
        );
        assert_eq!(&encoded[encoded.len() - 2..], &[0x00, 0x00], "terminator");
    }

    #[test]
    fn decoding_round_trips_an_encoded_message() {
        let mut decoder = Decoder::new();
        decoder.feed(&encode(&[1, 2, 3, 4, 5]));
        assert_eq!(decoder.next_message(1024), Ok(Some(vec![1, 2, 3, 4, 5])));
    }

    #[test]
    fn a_message_split_across_two_chunks_reassembles_in_order() {
        let mut wire = Vec::new();
        wire.extend_from_slice(&[0x00, 0x03]);
        wire.extend_from_slice(&[1, 2, 3]);
        wire.extend_from_slice(&[0x00, 0x02]);
        wire.extend_from_slice(&[4, 5]);
        wire.extend_from_slice(&[0x00, 0x00]);

        let mut decoder = Decoder::new();
        decoder.feed(&wire);
        assert_eq!(decoder.next_message(1024), Ok(Some(vec![1, 2, 3, 4, 5])));
    }

    #[test]
    fn feeding_bytes_one_at_a_time_still_assembles_correctly() {
        let wire = encode(&[9, 8, 7]);
        let mut decoder = Decoder::new();
        let mut result = None;
        for byte in wire {
            decoder.feed(&[byte]);
            if let Some(message) = decoder.next_message(1024).unwrap() {
                result = Some(message);
            }
        }
        assert_eq!(result, Some(vec![9, 8, 7]));
    }

    #[test]
    fn a_partial_chunk_header_is_truncation_not_an_error() {
        let mut decoder = Decoder::new();
        decoder.feed(&[0x00]);
        assert_eq!(decoder.next_message(1024), Ok(None));
    }

    #[test]
    fn a_declared_chunk_length_with_missing_data_is_truncation_not_an_error() {
        let mut decoder = Decoder::new();
        decoder.feed(&[0x00, 0x05, 1, 2]);
        assert_eq!(decoder.next_message(1024), Ok(None));
    }

    #[test]
    fn two_messages_back_to_back_are_each_yielded_once() {
        let mut wire = encode(&[1, 2]);
        wire.extend(encode(&[3, 4, 5]));
        let mut decoder = Decoder::new();
        decoder.feed(&wire);
        assert_eq!(decoder.next_message(1024), Ok(Some(vec![1, 2])));
        assert_eq!(decoder.next_message(1024), Ok(Some(vec![3, 4, 5])));
        assert_eq!(decoder.next_message(1024), Ok(None));
    }

    #[test]
    fn a_noop_empty_chunk_between_messages_yields_an_empty_message() {
        // The spec permits an empty chunk as a keep-alive; from the framing
        // layer's point of view that is indistinguishable from an
        // intentionally empty message, and interpreting it is the next
        // layer's job, not this one's.
        let mut wire = encode(&[1]);
        wire.extend_from_slice(&[0x00, 0x00]);
        wire.extend(encode(&[2]));
        let mut decoder = Decoder::new();
        decoder.feed(&wire);
        assert_eq!(decoder.next_message(1024), Ok(Some(vec![1])));
        assert_eq!(decoder.next_message(1024), Ok(Some(vec![])));
        assert_eq!(decoder.next_message(1024), Ok(Some(vec![2])));
    }

    #[test]
    fn an_oversized_message_is_refused_before_the_offending_chunk_is_appended() {
        let mut decoder = Decoder::new();
        decoder.feed(&[0x00, 0x64]); // declares a 100-byte chunk
        decoder.feed(&[0u8; 100]);
        assert_eq!(
            decoder.next_message(50),
            Err(ChunkError::MessageTooLarge { limit: 50 })
        );
    }

    #[test]
    fn a_message_exactly_at_the_budget_is_accepted_not_refused() {
        // The boundary itself must not be swept into the rejection by an
        // off-by-one — only *exceeding* the budget is refused, the same
        // property packstream's own allocation guard holds.
        let mut decoder = Decoder::new();
        let message = vec![7u8; 50];
        decoder.feed(&encode(&message));
        assert_eq!(decoder.next_message(50), Ok(Some(message)));
    }

    #[test]
    fn a_message_built_from_several_chunks_that_individually_fit_but_together_exceed_the_budget_is_refused()
     {
        let mut decoder = Decoder::new();
        decoder.feed(&[0x00, 0x1E]); // 30 bytes
        decoder.feed(&[0u8; 30]);
        decoder.feed(&[0x00, 0x1E]); // another 30 — 60 total, budget is 50
        decoder.feed(&[0u8; 30]);
        assert_eq!(
            decoder.next_message(50),
            Err(ChunkError::MessageTooLarge { limit: 50 })
        );
    }
}
