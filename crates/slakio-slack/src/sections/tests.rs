use super::*;
use serde_json::json;

fn section(id: &str, next: Option<&str>) -> Value {
    json!({"channel_section_id": id, "type": "standard", "next_channel_section_id": next})
}

#[test]
fn the_top_level_cursor_is_never_a_next_page() {
    let mut p = Paging::default();
    let page = json!({"channel_sections": [section("L1", Some("L2"))], "cursor": "always-here", "count": 9});
    assert_eq!(p.next(&page), Next::Done("no-cursor"), "an open chain and a top-level cursor ask no page more");
}

#[test]
fn only_response_metadata_names_a_next_page_while_the_chain_is_open() {
    let mut p = Paging::default();
    let first = json!({"channel_sections": [section("L1", Some("L2"))], "cursor": "top",
        "response_metadata": {"next_cursor": "page-2"}});
    assert_eq!(p.next(&first), Next::Page("page-2".to_string()));
    let second = json!({"channel_sections": [section("L2", None)], "response_metadata": {"next_cursor": "page-3"}});
    assert_eq!(p.next(&second), Next::Done("chain-closed"), "every section named was read");
}

#[test]
fn the_count_reached_stops_even_with_a_next_cursor() {
    let mut p = Paging::default();
    let page = json!({"channel_sections": [section("L1", Some("L9")), section("L2", None)], "count": 2,
        "response_metadata": {"next_cursor": "more"}});
    assert_eq!(p.next(&page), Next::Done("count-reached"));
}

#[test]
fn a_page_with_no_new_section_stops() {
    let mut p = Paging::default();
    let page = json!({"channel_sections": [section("L1", Some("L2"))], "response_metadata": {"next_cursor": "same"}});
    assert_eq!(p.next(&page), Next::Page("same".to_string()));
    assert_eq!(p.next(&page), Next::Done("no-new-section"), "the same page again");
    let mut empty = Paging::default();
    assert_eq!(
        empty.next(&json!({"channel_sections": [], "response_metadata": {"next_cursor": "x"}})),
        Next::Done("no-new-section")
    );
}

#[test]
fn a_cycle_of_sections_is_a_closed_chain() {
    let mut p = Paging::default();
    let page = json!({"channel_sections": [section("L1", Some("L2")), section("L2", Some("L1"))],
        "response_metadata": {"next_cursor": "more"}});
    assert_eq!(p.next(&page), Next::Done("chain-closed"));
}
