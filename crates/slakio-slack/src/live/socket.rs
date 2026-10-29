//! The live updates' socket after it opened: WebSocket frames (RFC 6455) read and written by
//! hand, the few the client needs. It reads text messages (a fragmented one put together),
//! answers a ping with a pong and a close with a close — the only frames it ever writes, which
//! the protocol needs to keep the socket open and to end it — and leaves binary messages and
//! pongs unread. Every frame it writes is masked, as a client's must be.
//!
//! Anything the protocol forbids a server to send fails the socket: a masked frame, reserved
//! bits (no extension was asked for), a control frame that is fragmented or longer than 125
//! bytes, a close frame with a code or reason it may not carry, a continuation out of place, text
//! that is not UTF-8, a message longer than [`MESSAGE_MAX`] (counting its data frames only).

use crate::error::SlackError;
use crate::request::{Ids, RandomIds};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufStream};

/// The longest message read; a longer one fails the socket (an event is a few kilobytes).
pub const MESSAGE_MAX: usize = 4 * 1024 * 1024;

/// What the server sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Incoming {
    Text(String),
    /// A ping (answered) or a pong: the socket is alive.
    Alive,
    /// The server ended the socket (answered with a close).
    Closed,
}

const TEXT: u8 = 0x1;
const BINARY: u8 = 0x2;
const CONTINUATION: u8 = 0x0;
const CLOSE: u8 = 0x8;
const PING: u8 = 0x9;
const PONG: u8 = 0xA;

/// An open socket on `IO`.
pub(crate) struct Socket<IO> {
    io: BufStream<IO>,
    ids: RandomIds,
}

fn broken(detail: &str) -> SlackError {
    SlackError::Network { detail: format!("live updates: {detail}") }
}

impl<IO: AsyncRead + AsyncWrite + Unpin> Socket<IO> {
    pub(crate) fn new(io: IO) -> Self {
        Self { io: BufStream::new(io), ids: RandomIds::default() }
    }

    /// The next text message, or word that the socket is alive (a ping, answered, or a pong);
    /// what is not text is left out on the way.
    pub(crate) async fn next(&mut self) -> Result<Incoming, SlackError> {
        let mut message: Option<(u8, Vec<u8>)> = None;
        loop {
            let (fin, opcode, payload) = self.frame(message.as_ref().map_or(0, |(_, m)| m.len())).await?;
            match opcode {
                PING => {
                    self.write(PONG, &payload).await?;
                    if message.is_none() {
                        return Ok(Incoming::Alive);
                    }
                }
                PONG => {
                    if message.is_none() {
                        return Ok(Incoming::Alive);
                    }
                }
                CLOSE => {
                    // The close is answered with the code it carried (none when it had none); one
                    // the protocol forbids is answered as a protocol error, and fails the socket.
                    let code = payload.get(..2).map(|c| u16::from_be_bytes([c[0], c[1]]));
                    let valid = std::str::from_utf8(payload.get(2..).unwrap_or_default()).is_ok()
                        && payload.len() != 1
                        && code.is_none_or(|c| matches!(c, 1000..=1003 | 1007..=1014 | 3000..=4999));
                    if !valid {
                        let _ = self.write(CLOSE, &1002u16.to_be_bytes()).await;
                        return Err(broken("a close frame the protocol forbids"));
                    }
                    let _ = self.write(CLOSE, payload.get(..2).unwrap_or_default()).await;
                    return Ok(Incoming::Closed);
                }
                TEXT | BINARY if message.is_none() => message = Some((opcode, payload)),
                CONTINUATION if message.is_some() => {
                    if let Some((_, m)) = message.as_mut() {
                        m.extend_from_slice(&payload);
                    }
                }
                _ => return Err(broken("a frame out of place")),
            }
            // A whole message: text is read, binary is not.
            if fin
                && opcode < CLOSE
                && let Some((TEXT, bytes)) = message.take()
            {
                return String::from_utf8(bytes).map(Incoming::Text).map_err(|_| broken("text not UTF-8"));
            }
        }
    }

    /// Close the socket from this end (the code for a normal close), without waiting.
    pub(crate) async fn close(&mut self) {
        let _ = self.write(CLOSE, &1000u16.to_be_bytes()).await;
    }

    /// One frame: whether it ends its message, its opcode and payload. `so_far`: the bytes of
    /// the message read before it.
    async fn frame(&mut self, so_far: usize) -> Result<(bool, u8, Vec<u8>), SlackError> {
        let lost = |_| broken("the connection ended");
        let mut head = [0u8; 2];
        self.io.read_exact(&mut head).await.map_err(lost)?;
        let (fin, rsv, opcode) = (head[0] & 0x80 != 0, head[0] & 0x70, head[0] & 0x0F);
        if rsv != 0 {
            return Err(broken("reserved bits set"));
        }
        if head[1] & 0x80 != 0 {
            return Err(broken("a masked frame from the server"));
        }
        let len = match head[1] & 0x7F {
            126 => u64::from(self.io.read_u16().await.map_err(lost)?),
            127 => self.io.read_u64().await.map_err(lost)?,
            n => u64::from(n),
        };
        let control = opcode >= CLOSE;
        if control && (!fin || len > 125) {
            return Err(broken("a control frame fragmented or too long"));
        }
        // A control frame between a message's fragments is not part of it.
        let so_far = if control { 0 } else { so_far };
        let len = usize::try_from(len).ok().filter(|n| so_far.saturating_add(*n) <= MESSAGE_MAX);
        let Some(len) = len else { return Err(broken("a message too long")) };
        let mut payload = vec![0u8; len];
        self.io.read_exact(&mut payload).await.map_err(lost)?;
        Ok((fin, opcode, payload))
    }

    /// Write one whole frame of `opcode` carrying `payload` (a control frame's: ≤ 125 bytes),
    /// masked.
    async fn write(&mut self, opcode: u8, payload: &[u8]) -> Result<(), SlackError> {
        let mask = self.ids.hex(4);
        let mask = u32::from_str_radix(&mask, 16).unwrap_or(0).to_be_bytes();
        let len = u8::try_from(payload.len()).ok().filter(|n| *n <= 125).ok_or_else(|| broken("too long"))?;
        let mut frame = vec![0x80 | opcode, 0x80 | len];
        frame.extend_from_slice(&mask);
        frame.extend(payload.iter().zip(mask.iter().cycle()).map(|(b, m)| b ^ m));
        let lost = |_| broken("the connection ended");
        self.io.write_all(&frame).await.map_err(lost)?;
        self.io.flush().await.map_err(lost)
    }
}

/// What the server must answer to `key` (`Sec-WebSocket-Accept`): the base64 of the SHA-1 of
/// the key and the protocol's own GUID.
pub(crate) fn accept(key: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(key.as_bytes());
    h.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    base64(&h.finalize())
}

/// Standard base64 with padding.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ABC[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
