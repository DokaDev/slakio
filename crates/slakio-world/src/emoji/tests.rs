use super::*;
use slakio_core::images::emoji::{Custom, resolve};
use slakio_core::images::emoji_address;

#[test]
fn the_world_s_emoji_resolve_as_slack_s_would() {
    let r = resolve(&list("TDEMOA"));
    let names: Vec<&str> = r.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["blob-dance", "party-blob", "ship-it", "yes"]);
    assert_eq!(r[0].1, Custom::Picture(picture("TDEMOA", "party-blob")));
    assert_eq!(r[3].1, Custom::Standard("white_check_mark".into()));
    let p = picture("TDEMOA", "party-blob");
    assert!(emoji_address(&p).is_some());
    let path = p.split_once(".com").unwrap().1;
    assert!(is_path(path), "{path}");
    assert_eq!(crate::pictures::served(path).map(|b| b[16..24].to_vec()), Some(vec![0, 0, 0, 64, 0, 0, 0, 64]));
    for other in ["/avatars/T/U-72.png", "/files-tmb/T-F1/a.png", "/api/emoji.list", "/TDEMOA/x/zz.png", "/t/x/ab.png"]
    {
        assert!(!is_path(other), "{other}");
    }
}
