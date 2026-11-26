use super::*;
use slakio_core::i18n::Lang;
use slakio_core::model::UserId;
use slakio_core::sanitize::Remote;
use slakio_world::World;

fn names(n: usize) -> Vec<String> {
    ["Ann", "Bob", "Cy", "Dee", "Eve", "Fay", "Gus", "Hal"].iter().take(n).map(ToString::to_string).collect()
}

fn en() -> I18n {
    I18n::new(Lang::En)
}

fn ko() -> I18n {
    I18n::new(Lang::Ko)
}

#[test]
fn two_people_are_both_named() {
    assert_eq!(short(&names(1), &en()), "Ann");
    assert_eq!(short(&names(2), &en()), "Ann, Bob");
    assert_eq!(short(&names(2), &ko()), "Ann, Bob");
}

#[test]
fn eight_people_show_two_and_the_rest() {
    assert_eq!(short(&names(8), &en()), "Ann, Bob and 6 others");
    assert_eq!(short(&names(8), &ko()), "Ann, Bob \u{c678} 6\u{ba85}");
    assert_eq!(short(&names(3), &en()), "Ann, Bob and 1 other");
    assert_eq!(short(&names(3), &ko()), "Ann, Bob \u{c678} 1\u{ba85}");
}

#[test]
fn fit_keeps_the_suffix_whole_in_narrow_rows() {
    let all = names(8);
    assert_eq!(fit(&all, &en(), 40), "Ann, Bob and 6 others");
    assert_eq!(fit(&all, &en(), 20), "Ann and 7 others");
    assert_eq!(fit(&all, &en(), 15), "A\u{2026} and 7 others");
    assert_eq!(fit(&all, &ko(), 15), "Ann, Bob \u{c678} 6\u{ba85}");
    assert_eq!(fit(&all, &ko(), 10), "Ann \u{c678} 7\u{ba85}");
    // No room for a name beside the suffix: the text is cut as a whole.
    assert_eq!(fit(&all, &en(), 8), clip("Ann and 7 others", 8));
    // Up to two names are cut as one text, as before.
    assert_eq!(fit(&names(2), &en(), 6), clip("Ann, Bob", 6));
}

#[test]
fn the_count_says_up_to_nine_then_nine_plus() {
    assert_eq!((count(1), count(9), count(10), count(12)), ("1".into(), "9".into(), "9+".into(), "9+".into()));
}

#[test]
fn a_group_dm_named_by_its_people_is_named_in_short_and_another_keeps_its_name() {
    let mut snapshot = World::demo().snapshot().clone();
    let at = snapshot.conversations.iter().position(|c| matches!(c.kind, ConversationKind::GroupDm { .. })).unwrap();
    let model = Model::new(snapshot.clone());
    let c = &model.conversations()[at];
    let people = people(&model, c).expect("the demo group DM is named by its people");
    assert_eq!(people.join(", "), c.name.unsanitized());
    // A name of Slack's handles (its people not all known): as it is.
    snapshot.conversations[at].name = Remote::new("ann, bob, cy");
    let model = Model::new(snapshot.clone());
    let c = &model.conversations()[at];
    assert_eq!(people_of(&model, at), None);
    assert_eq!(name(&model, &en(), c), "ann, bob, cy");
    // Someone not known: as it is too.
    let ConversationKind::GroupDm { users } = &mut snapshot.conversations[at].kind else { unreachable!() };
    users.push(UserId::new("U-nobody"));
    let model = Model::new(snapshot);
    assert_eq!(people_of(&model, at), None);
}

fn people_of(model: &Model, at: usize) -> Option<Vec<String>> {
    people(model, &model.conversations()[at])
}
