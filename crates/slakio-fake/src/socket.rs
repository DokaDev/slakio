//! The fake's live updates socket: an opening request (`Upgrade: websocket`) is answered `101`
//! with the key's digest, then `{"type":"hello"}` is sent and the test pushes synthetic events
//! ([`crate::FakeSlack::push`]), pings, drops or closes it. What the client sends over it is kept
//! (each frame's opcode and unmasked payload), so a test can check it sends nothing but pongs and
//! a close. Frames are read and written by hand (RFC 6455), as a server does: its own unmasked,
//! the client's masked.

use crate::{Logged, Shared};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::OwnedReadHalf;
use tokio::sync::mpsc;

/// What a test does to every open socket.
#[derive(Clone, Debug)]
pub(crate) enum Push {
    Text(String),
    Ping(Vec<u8>),
    /// Close with a close frame, as Slack does before it goes away.
    Close,
    /// End the connection without a word (a network that went away).
    Drop,
}

/// A frame the client sent: its opcode and payload, unmasked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientFrame {
    pub opcode: u8,
    pub payload: Vec<u8>,
    /// It was masked, as every client frame must be.
    pub masked: bool,
}

/// The digest the server answers `key` with.
fn accept(key: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(key.as_bytes());
    h.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let bytes = h.finalize();
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            out.push(if i <= chunk.len() { char::from(ABC[((n >> (18 - 6 * i)) & 63) as usize]) } else { '=' });
        }
    }
    out
}

/// Serve the socket asked for by `logged` on `stream`.
pub(crate) async fn serve(mut stream: TcpStream, logged: Logged, shared: Arc<Mutex<Shared>>) {
    let header = |name: &str| logged.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
    let (refused, hello) = {
        let mut s = shared.lock().unwrap_or_else(PoisonError::into_inner);
        s.upgrades.push(logged.clone());
        let refused = s.refuse_sockets > 0;
        s.refuse_sockets = s.refuse_sockets.saturating_sub(1);
        (refused, s.hello)
    };
    let Some(key) = header("sec-websocket-key").filter(|_| !refused) else {
        let _ = stream.write_all(b"HTTP/1.1 503 FAKE\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await;
        return;
    };
    let head = format!(
        "HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\nconnection: Upgrade\r\nsec-websocket-accept: {}\r\n\r\n",
        accept(&key)
    );
    if stream.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    let (rx, mut tx) = stream.into_split();
    let (push_tx, mut push_rx) = mpsc::unbounded_channel();
    shared.lock().unwrap_or_else(PoisonError::into_inner).sockets.push(push_tx);
    if hello && tx.write_all(&frame(0x1, br#"{"type":"hello"}"#)).await.is_err() {
        return;
    }
    let mut reader = tokio::spawn(read_frames(rx, Arc::clone(&shared)));
    loop {
        let push = tokio::select! {
            push = push_rx.recv() => push,
            // The client went away.
            _ = &mut reader => break,
        };
        let Some(push) = push else { break };
        let out = match push {
            Push::Text(t) => frame(0x1, t.as_bytes()),
            Push::Ping(p) => frame(0x9, &p),
            Push::Close => frame(0x8, &1001u16.to_be_bytes()),
            Push::Drop => break,
        };
        if tx.write_all(&out).await.is_err() {
            break;
        }
    }
    reader.abort();
}

/// A server's frame: unmasked, whole.
fn frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0x80 | opcode];
    match payload.len() {
        n @ 0..=125 => out.push(u8::try_from(n).unwrap_or(0)),
        n @ 126..=0xFFFF => {
            out.push(126);
            out.extend_from_slice(&u16::try_from(n).unwrap_or(0).to_be_bytes());
        }
        n => {
            out.push(127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    out.extend_from_slice(payload);
    out
}

/// Keep every frame the client sends, until it closes.
async fn read_frames(mut rx: OwnedReadHalf, shared: Arc<Mutex<Shared>>) {
    loop {
        let mut head = [0u8; 2];
        if rx.read_exact(&mut head).await.is_err() {
            return;
        }
        let masked = head[1] & 0x80 != 0;
        let len = match head[1] & 0x7F {
            126 => rx.read_u16().await.map(u64::from),
            127 => rx.read_u64().await,
            n => Ok(u64::from(n)),
        };
        let Ok(len) = len.map(|n| usize::try_from(n).unwrap_or(usize::MAX)) else { return };
        if len > 1 << 20 {
            return;
        }
        let mut mask = [0u8; 4];
        if masked && rx.read_exact(&mut mask).await.is_err() {
            return;
        }
        let mut payload = vec![0u8; len];
        if rx.read_exact(&mut payload).await.is_err() {
            return;
        }
        if masked {
            for (b, m) in payload.iter_mut().zip(mask.iter().cycle()) {
                *b ^= m;
            }
        }
        let opcode = head[0] & 0x0F;
        shared.lock().unwrap_or_else(PoisonError::into_inner).client_frames.push(ClientFrame {
            opcode,
            payload,
            masked,
        });
    }
}
