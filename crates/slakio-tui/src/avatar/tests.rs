use super::*;
use crate::text::width;

#[test]
fn latin_names_give_the_first_and_last_initial() {
    assert_eq!(initials("Minsu Kim", || "minsu.kim".into()), "MK");
    assert_eq!(initials("alex de la morgan", || "alex".into()), "AM", "first and last word, upper case");
    assert_eq!(initials("Hana", || "hana".into()), "H ", "one word: one letter and a space");
    assert_eq!(initials("  Jiho   Park  ", || "j".into()), "JP");
    assert_eq!(initials("(Sam) Rivera", || "sam".into()), "SR", "marks before a letter are skipped");
    assert_eq!(initials("Zoë Ünal", || "z".into()), "ZÜ");
    assert_eq!(initials("Deploy Bot 2", || "deploybot".into()), "D2", "a digit counts");
}

#[test]
fn a_name_starting_wide_gives_its_first_syllable() {
    // Hangul and other CJK: the first syllable (the family name), two cells.
    assert_eq!(initials("\u{C774}\u{C11C}\u{C5F0}", || "seoyeon.lee".into()), "\u{C774}");
    assert_eq!(initials("\u{BC15} \u{C900}\u{D638}", || "junho".into()), "\u{BC15}");
    assert_eq!(initials("\u{5C71}\u{7530}\u{592A}\u{90CE}", || "yamada".into()), "\u{5C71}");
    // A narrow first initial never pairs with a wide one: it would take three cells.
    assert_eq!(initials("Kim \u{BBFC}\u{C218}", || "k".into()), "K ");
    for name in ["Minsu Kim", "Hana", "\u{C774}\u{C11C}\u{C5F0}", "Kim \u{BBFC}\u{C218}", "", "\u{1F600}"] {
        assert_eq!(width(&initials(name, || "x".into())), WIDTH, "{name:?}");
    }
}

#[test]
fn a_name_without_letters_falls_back_to_the_handle_then_a_question_mark() {
    assert_eq!(initials("\u{1F600} \u{2728}", || "robin.choi".into()), "RC", "emoji are not initials");
    assert_eq!(initials("", || "dana".into()), "D ");
    assert_eq!(initials("---", || "...".into()), "? ");
    // What the sanitiser leaves of control characters is not a letter either.
    assert_eq!(initials("\u{FFFD}Mallory \u{FFFD}", || "m".into()), "M ");
    // Combining marks and zero-width characters never reach the chip.
    assert_eq!(initials("\u{0301}\u{200B}eve", || "e".into()), "E ");
}

#[test]
fn a_persons_slot_is_stable_and_spreads_people_out() {
    let id = |s: &str| UserId::new(s);
    assert_eq!(slot(&id("U1")), slot(&id("U1")));
    // FNV-1a of "U1": fixed, so a person keeps their color across versions.
    assert_eq!(slot(&id("U1")), 0x03F2_F613_usize);
    let slots: std::collections::HashSet<usize> =
        (0..12).map(|i| slot(&id(&format!("UDEMOA{:03}", i + 1))) % 8).collect();
    assert!(slots.len() >= 5, "twelve people use most of eight colors: {slots:?}");
}

#[test]
fn initials_fill_a_slot_on_its_first_row_and_the_rest_stays_blank() {
    let art = Art::Initials("MK".to_string());
    assert_eq!((art.row(BLOCK, 0), art.row(BLOCK, 1)), (" MK ".to_string(), "    ".to_string()));
    assert_eq!(art.row(CHIP, 0), "MK");
    let hangul = Art::Initials("\u{AE40}".to_string());
    assert_eq!(width(&hangul.row(BLOCK, 0)), 4, "a wide syllable keeps the slot's width");
}
