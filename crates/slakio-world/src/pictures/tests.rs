use super::*;
use slakio_core::images::{avatar_address, thumb_address};

#[test]
fn a_picture_is_a_png_of_its_size_and_the_same_every_time() {
    let p = png("x", 7, 3);
    assert_eq!(&p[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&p[16..24], &[0, 0, 0, 7, 0, 0, 0, 3]);
    assert_eq!(p, png("x", 7, 3));
    assert_ne!(p, png("y", 7, 3));
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926, "the check value of CRC-32");
}

#[test]
fn the_addresses_pass_the_checks_slack_s_do() {
    let ws = slakio_core::model::WorkspaceId::new("TDEMO1");
    let photos: Vec<String> = (0..30).filter_map(|i| avatar(&ws, &format!("U{i}"), false)).collect();
    assert!(photos.len() > 10 && photos.len() < 30, "most people have a photo, not all");
    assert!(photos.iter().all(|a| avatar_address(a).is_some()));
    assert_eq!(avatar(&ws, "U1", true), None, "bots have none");
    let t = thumb("TDEMO1", "F1", "login-screen.png").unwrap();
    assert!(thumb_address(&t).is_some());
    assert_eq!(thumb("TDEMO1", "F1", "notes.txt"), None);
    let path = |a: &str| a.split_once(".com").unwrap().1.to_string();
    assert_eq!(served(&path(&photos[0])).map(|p| p[16..24].to_vec()), Some(vec![0, 0, 0, 72, 0, 0, 0, 72]));
    assert!(served(&path(&t)).is_some());
    assert_eq!(served("/files-pri/x"), None);
}
