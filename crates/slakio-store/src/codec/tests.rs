use super::*;

#[test]
fn packed_texts_come_back_as_they_were_whatever_they_hold() {
    let cases: Vec<Vec<&str>> = vec![
        vec![],
        vec![""],
        vec!["U1", "U2"],
        vec!["a:b", "3:xyz", ""],
        vec!["\u{1F44D}", "tone:skin-tone-2", "new\nline"],
    ];
    for parts in cases {
        assert_eq!(unpack(&pack(&parts)), Some(parts.iter().map(ToString::to_string).collect()), "{parts:?}");
    }
}

#[test]
fn a_damaged_pack_is_none_not_a_guess() {
    for bad in ["x", "3:ab", "2", "-1:a", "1:é", "2:ab3"] {
        assert_eq!(unpack(bad), None, "{bad:?}");
    }
}

#[test]
fn numbers_that_do_not_fit_are_errors() {
    assert_eq!(int(1_767_603_600_000_100).unwrap(), 1_767_603_600_000_100);
    assert!(int(u64::MAX).is_err());
    assert!(uint(-1).is_err());
    assert!(count(i64::from(u32::MAX) + 1).is_err());
    assert_eq!(count(7).unwrap(), 7);
}
