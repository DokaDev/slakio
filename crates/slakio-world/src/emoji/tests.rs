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

#[test]
fn a_name_of_another_script_is_a_path_once_decoded() {
    let path = "/TDEMOA/%EA%B0%90%EC%82%AC/00ab.png";
    assert!(is_path(path), "{path}");
    assert!(crate::pictures::served(path).is_some());
    for bad in
        ["/TDEMOA/%EA%B0/00ab.png", "/TDEMOA/%zz/00ab.png", "/TDEMOA/%E1%84%80/00ab.png", "/TDEMOA/a%3Ab/00ab.png"]
    {
        assert!(!is_path(bad), "{bad}");
    }
}
