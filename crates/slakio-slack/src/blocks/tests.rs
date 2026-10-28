use super::*;
use serde_json::json;

fn rich(elements: Value) -> Value {
    json!([{"type": "rich_text", "elements": elements}])
}

#[test]
fn rich_text_only_is_the_message_s_text() {
    let blocks = rich(json!([{"type": "rich_text_section", "elements": [{"type": "text", "text": "other words"}]}]));
    assert_eq!(text("*the* text", Some(&blocks), None, &[]), "*the* text");
}

#[test]
fn rich_text_is_written_out_when_the_text_is_empty() {
    let blocks = rich(json!([
        {"type": "rich_text_section", "elements": [
            {"type": "text", "text": "Hi "},
            {"type": "user", "user_id": "U1"},
            {"type": "text", "text": " see "},
            {"type": "text", "text": "this ", "style": {"bold": true}},
            {"type": "link", "url": "https://x.example/a|b", "text": "doc <1>"},
            {"type": "text", "text": " in "},
            {"type": "channel", "channel_id": "C2"},
            {"type": "emoji", "name": "eyes"},
            {"type": "broadcast", "range": "here"},
            {"type": "text", "text": "a<b>&c", "style": {"code": true}}
        ]},
        {"type": "rich_text_list", "style": "ordered", "elements": [
            {"type": "rich_text_section", "elements": [{"type": "text", "text": "one"}]},
            {"type": "rich_text_section", "elements": [{"type": "text", "text": "two"}]}
        ]},
        {"type": "rich_text_quote", "elements": [{"type": "text", "text": "said\nso"}]},
        {"type": "rich_text_preformatted", "elements": [{"type": "text", "text": "x = 1"}]}
    ]));
    assert_eq!(
        text("", Some(&blocks), None, &[]),
        "Hi <@U1> see *this* <https://x.example/a%7Cb|doc &lt;1&gt;> in <#C2>:eyes:<!here>`a&lt;b&gt;&amp;c`\n\
         1. one\n2. two\n&gt; said\n&gt; so\n```\nx = 1\n```"
    );
}

#[test]
fn a_bot_s_sections_replace_its_fallback() {
    let blocks = json!([
        {"type": "header", "text": {"type": "plain_text", "text": "Alert <firing>"}},
        {"type": "section", "text": {"type": "mrkdwn", "text": "*CPU* high"},
         "fields": [{"type": "mrkdwn", "text": "*Host*\nweb-1"}]},
        {"type": "divider"},
        {"type": "context", "elements": [{"type": "mrkdwn", "text": "from grafana"}, {"type": "image", "alt_text": "logo"}]},
        {"type": "actions", "elements": [{"type": "button", "text": {"type": "plain_text", "text": "Ack"}}]}
    ]);
    assert_eq!(
        text("Alert firing (fallback)", Some(&blocks), None, &[]),
        "*Alert &lt;firing&gt;*\n*CPU* high\n*Host*\nweb-1\nfrom grafana 🖼 logo"
    );
}

#[test]
fn attachments_and_files_follow_the_text() {
    let attachments = json!([
        {"pretext": "Deploy", "title": "Build 42", "title_link": "https://ci.example/42", "text": "passed",
         "fields": [{"title": "Branch", "value": "main"}]},
        {"fallback": "only a fallback"}
    ]);
    assert_eq!(
        text("see", None, Some(&attachments), &["report.pdf".to_string()]),
        "see\nDeploy\n*<https://ci.example/42|Build 42>*\npassed\n*Branch*\nmain\nonly a fallback\n📎 report.pdf"
    );
}

#[test]
fn unknown_and_malformed_blocks_are_left_out() {
    let blocks = json!([{"type": "something_new"}, 7, {"type": "section"}, {"type": "rich_text", "elements": "no"}]);
    assert_eq!(text("fallback", Some(&blocks), None, &[]), "fallback");
    assert_eq!(text("", Some(&json!("not a list")), Some(&json!({"x": 1})), &[]), "");
}

#[test]
fn deeply_nested_rich_text_stops_reading() {
    let mut v = json!([{"type": "text", "text": "deep"}]);
    for _ in 0..50 {
        v = json!([{"type": "rich_text_unknown", "elements": v}]);
    }
    assert_eq!(text("", Some(&rich(v)), None, &[]), "");
}

#[test]
fn ids_cannot_close_their_entity() {
    let blocks = rich(json!([{"type": "rich_text_section", "elements": [{"type": "user", "user_id": "U1>|<!here"}]}]));
    assert_eq!(text("", Some(&blocks), None, &[]), "<@U1here>");
}
