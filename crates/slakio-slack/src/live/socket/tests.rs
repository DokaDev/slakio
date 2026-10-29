use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex};

/// A server's frame: unmasked.
fn frame(fin: bool, opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![if fin { 0x80 } else { 0 } | opcode];
    match payload.len() {
        n @ 0..=125 => out.push(u8::try_from(n).unwrap()),
        n @ 126..=0xFFFF => {
            out.push(126);
            out.extend_from_slice(&u16::try_from(n).unwrap().to_be_bytes());
        }
        n => {
            out.push(127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    out.extend_from_slice(payload);
    out
}

/// A client frame the socket wrote, unmasked: (opcode, payload); it must be masked and whole.
async fn written(server: &mut DuplexStream) -> (u8, Vec<u8>) {
    let mut head = [0u8; 2];
    server.read_exact(&mut head).await.unwrap();
    assert_eq!(head[0] & 0x80, 0x80, "whole");
    assert_eq!(head[1] & 0x80, 0x80, "masked");
    let len = usize::from(head[1] & 0x7F);
    let mut mask = [0u8; 4];
    server.read_exact(&mut mask).await.unwrap();
    let mut payload = vec![0u8; len];
    server.read_exact(&mut payload).await.unwrap();
    for (b, m) in payload.iter_mut().zip(mask.iter().cycle()) {
        *b ^= m;
    }
    (head[0] & 0x0F, payload)
}

fn pair() -> (Socket<DuplexStream>, DuplexStream) {
    let (client, server) = duplex(1 << 16);
    (Socket::new(client), server)
}

#[tokio::test]
async fn text_is_read_whole_and_fragments_put_together_binary_left_out() {
    let (mut s, mut server) = pair();
    server.write_all(&frame(true, TEXT, b"{\"type\":\"hello\"}")).await.unwrap();
    server.write_all(&frame(true, BINARY, b"\x00\x01")).await.unwrap();
    server.write_all(&frame(false, TEXT, "漢字".as_bytes())).await.unwrap();
    server.write_all(&frame(false, CONTINUATION, b" and ")).await.unwrap();
    server.write_all(&frame(true, CONTINUATION, &[b'x'; 300])).await.unwrap();
    assert_eq!(s.next().await.unwrap(), Incoming::Text("{\"type\":\"hello\"}".into()));
    let joined = format!("漢字 and {}", "x".repeat(300));
    assert_eq!(s.next().await.unwrap(), Incoming::Text(joined));
}

#[tokio::test]
async fn a_ping_is_answered_with_its_pong_and_a_close_with_a_close() {
    let (mut s, mut server) = pair();
    server.write_all(&frame(true, PING, b"keep")).await.unwrap();
    // A ping between fragments too.
    server.write_all(&frame(false, TEXT, b"ab")).await.unwrap();
    server.write_all(&frame(true, PING, b"")).await.unwrap();
    server.write_all(&frame(true, CONTINUATION, b"c")).await.unwrap();
    server.write_all(&frame(true, PONG, b"")).await.unwrap();
    server.write_all(&frame(true, CLOSE, &1001u16.to_be_bytes())).await.unwrap();
    assert_eq!(s.next().await.unwrap(), Incoming::Alive);
    // Within a message, a ping is answered and the message read on.
    assert_eq!(s.next().await.unwrap(), Incoming::Text("abc".into()));
    assert_eq!(s.next().await.unwrap(), Incoming::Alive, "a pong");
    assert_eq!(written(&mut server).await, (PONG, b"keep".to_vec()));
    assert_eq!(written(&mut server).await, (PONG, Vec::new()));
    assert_eq!(s.next().await.unwrap(), Incoming::Closed);
    assert_eq!(written(&mut server).await, (CLOSE, 1001u16.to_be_bytes().to_vec()));
}

#[tokio::test]
async fn closing_from_this_end_says_a_normal_close() {
    let (mut s, mut server) = pair();
    s.close().await;
    assert_eq!(written(&mut server).await, (CLOSE, 1000u16.to_be_bytes().to_vec()));
}

#[tokio::test]
async fn what_a_server_must_not_send_fails_the_socket() {
    let mut masked = frame(true, TEXT, b"hi");
    masked[1] |= 0x80;
    masked.splice(2..2, [1, 2, 3, 4]);
    let mut reserved = frame(true, TEXT, b"hi");
    reserved[0] |= 0x40;
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("masked", masked),
        ("reserved bits", reserved),
        ("fragmented ping", frame(false, PING, b"")),
        ("long ping", frame(true, PING, &[0; 126])),
        ("continuation first", frame(true, CONTINUATION, b"x")),
        ("text not UTF-8", frame(true, TEXT, &[0xFF, 0xFE])),
        ("unknown opcode", frame(true, 0x3, b"")),
        ("too long", {
            let mut f = vec![0x81, 127];
            f.extend_from_slice(&(MESSAGE_MAX as u64 + 1).to_be_bytes());
            f
        }),
    ];
    for (what, bytes) in cases {
        let (mut s, mut server) = pair();
        server.write_all(&bytes).await.unwrap();
        assert!(matches!(s.next().await, Err(SlackError::Network { .. })), "{what}");
    }
    // Fragments that together pass the limit.
    let (mut s, mut server) = pair();
    let half = vec![b'a'; MESSAGE_MAX / 2 + 1];
    tokio::spawn(async move {
        let _ = server.write_all(&frame(false, TEXT, &half)).await;
        let _ = server.write_all(&frame(true, CONTINUATION, &half)).await;
    });
    assert!(matches!(s.next().await, Err(SlackError::Network { .. })), "fragments too long together");
}

#[tokio::test]
async fn a_close_frame_the_protocol_forbids_fails_the_socket_with_a_protocol_error() {
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("one byte", vec![0x03]),
        ("reserved code", 1005u16.to_be_bytes().to_vec()),
        ("code below the range", 999u16.to_be_bytes().to_vec()),
        ("unassigned code", 2000u16.to_be_bytes().to_vec()),
        ("reason not UTF-8", [&1000u16.to_be_bytes()[..], &[0xFF]].concat()),
    ];
    for (what, payload) in cases {
        let (mut s, mut server) = pair();
        server.write_all(&frame(true, CLOSE, &payload)).await.unwrap();
        assert!(matches!(s.next().await, Err(SlackError::Network { .. })), "{what}");
        assert_eq!(written(&mut server).await, (CLOSE, 1002u16.to_be_bytes().to_vec()), "{what}");
    }
    // An empty close, and one with an application's code and a reason, are fine.
    for payload in [Vec::new(), [&4000u16.to_be_bytes()[..], b"bye"].concat()] {
        let (mut s, mut server) = pair();
        server.write_all(&frame(true, CLOSE, &payload)).await.unwrap();
        assert_eq!(s.next().await.unwrap(), Incoming::Closed);
        assert_eq!(written(&mut server).await.1, payload.get(..2).unwrap_or_default());
    }
}

#[tokio::test]
async fn a_ping_inside_a_message_at_its_limit_is_not_counted_on_it() {
    let (mut s, mut server) = pair();
    let most = vec![b'a'; MESSAGE_MAX - 10];
    let rest = vec![b'b'; 10];
    let expected = format!("{}{}", "a".repeat(MESSAGE_MAX - 10), "b".repeat(10));
    let writer = tokio::spawn(async move {
        server.write_all(&frame(false, TEXT, &most)).await.unwrap();
        server.write_all(&frame(true, PING, &[b'p'; 125])).await.unwrap();
        server.write_all(&frame(true, CONTINUATION, &rest)).await.unwrap();
        assert_eq!(written(&mut server).await, (PONG, vec![b'p'; 125]));
        server
    });
    let got = s.next().await.unwrap();
    assert!(got == Incoming::Text(expected), "the whole message, read on past the ping");
    drop(writer.await.unwrap());
}

#[tokio::test]
async fn an_ended_connection_is_an_error_not_a_message() {
    let (mut s, server) = pair();
    drop(server);
    assert!(matches!(s.next().await, Err(SlackError::Network { .. })));
}

#[test]
fn base64_and_the_accept_digest_match_the_standard() {
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    // RFC 6455, section 1.3.
    assert_eq!(accept("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
}
