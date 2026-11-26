//! More of the picture diagnostics against the fake Slack: the words of failures (the network,
//! the governor's shares), the lookups of the user's own photo, the sidebar probe that never
//! signs out, and caps on what an answer's shape may make the log write.

use serde_json::json;
use slakio_core::backend::{BackendHandle, Command, Envelope, Event, Generation, Request};
use slakio_core::diag::Diag;
use slakio_core::model::FILE_HOST;
use slakio_core::secret::Credentials;
use slakio_fake::{FakeSlack, Fault};
use slakio_slack::Client;
use slakio_slack::governor::{CdnLimits, FakeClock, Governor, Limits};
use slakio_slack::request::Endpoints;
use slakio_world::World;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const TOKEN: &str = concat!("xo", "xc-TEST");

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slakio-diag-more-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("state").join("pictures.log")
}

struct Run {
    fake: FakeSlack,
    handle: BackendHandle,
    diag: Diag,
    path: PathBuf,
    id: u64,
}

impl Run {
    /// The fake Slack, diagnostics on; requests go to `to` (the fake's own address when `None`).
    async fn new(name: &str, to: Option<SocketAddr>) -> Self {
        Self::limited(name, to, Limits::default()).await
    }

    /// [`Run::new`] with the governor's `limits`.
    async fn limited(name: &str, to: Option<SocketAddr>, limits: Limits) -> Self {
        Self::with_cdn(name, to, limits, CdnLimits::default()).await
    }

    async fn with_cdn(name: &str, to: Option<SocketAddr>, limits: Limits, cdn: CdnLimits) -> Self {
        let path = scratch(name);
        let diag = Diag::to(path.clone());
        let fake = FakeSlack::start(World::demo()).await.unwrap();
        let governor = Governor::with_cdn(limits, cdn, Box::new(FakeClock::default()), true);
        let endpoints = Endpoints::local(to.unwrap_or(fake.addr())).unwrap();
        let client = Client::new(Credentials::new(TOKEN, "cookie-TEST"), endpoints, Arc::clone(&governor))
            .with_diag(diag.clone());
        Self { fake, handle: slakio_slack::serve(client), diag, path, id: 0 }
    }

    async fn ask(&mut self, command: Command) -> Event {
        self.id += 1;
        self.handle.send(Request { id: Generation(self.id), workspace: None, command }).unwrap();
        let e: Envelope =
            tokio::time::timeout(Duration::from_secs(20), self.handle.rx.recv()).await.expect("an answer").unwrap();
        e.event
    }

    fn calls(&self, method: &str) -> usize {
        self.fake.log().iter().filter(|l| l.method() == method).count()
    }

    /// What the file holds now.
    fn text(&self) -> String {
        self.diag.flush();
        std::fs::read_to_string(&self.path).unwrap_or_default()
    }
}

fn has(text: &str, line: &str) {
    assert!(text.contains(line), "no `{line}` in\n{text}");
}

#[test]
fn a_host_that_cannot_be_reached_is_the_network() {
    rt().block_on(async {
        // An address nothing listens on: the connection is refused.
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let to = closed.local_addr().unwrap();
        drop(closed);
        let mut r = Run::new("network", Some(to)).await;
        assert!(matches!(r.ask(Command::Emoji).await, Event::Failed { .. }));
        let text = r.text();
        has(&text, "emoji-list failed network");
        assert!(!text.contains("failed other"), "{text}");
    });
}

#[test]
fn the_user_s_own_lookup_says_what_it_gave_and_the_answer_s_counts() {
    rt().block_on(async {
        let mut r = Run::new("lookup", None).await;
        r.fake.set_own_photo(false);
        let me = World::demo().snapshot().workspaces[0].me.as_str().to_string();
        r.fake.fail_next("users/info", Fault::Answer(json!({"ok": true, "results": [], "pending_ids": [me]})));
        assert!(matches!(r.ask(Command::Boot).await, Event::Booted(_)));
        let text = r.text();
        has(&text, "self-photo lookup result standing=unknown in_results=no photo=none");
        has(&text, "avatar counted lookup results=0 pending_ids=1 failed_ids=-");
        has(&text, "avatar field lookup_key name=pending_ids");
        has(&text, "self-photo source none");
        assert!(r.calls("users/info") >= 1);
    });
}

#[test]
fn a_lookup_answer_tells_its_entries_names_photo_hosts_and_what_it_decoded() {
    rt().block_on(async {
        let mut r = Run::new("lookup-shape", None).await;
        r.fake.set_own_photo(false);
        let me = World::demo().snapshot().workspaces[0].me.as_str().to_string();
        let original = "https://s3-us-west-2.amazonaws.com/slack-files2/avatars/2026-01-05/1234_SECRETFILE_original.jpg";
        let answer = json!({"ok": true, "results": [{"id": me, "name": "secretme", "updated": 1, "profile": {
            "real_name": "Secret Me", "image_original": original, "avatar_hash": "SECRETHASH", "is_custom_image": true}}]});
        r.fake.fail_next("users/info", Fault::Answer(answer));
        assert!(matches!(r.ask(Command::Boot).await, Event::Booted(_)));
        let text = r.text();
        for secret in ["SECRET", "secretme", "slack-files2", "1234_"] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
        has(&text, "avatar counted lookup results=1 ");
        for name in ["id", "name", "profile", "updated"] {
            has(&text, &format!("avatar field result_key name={name} entries=1"));
        }
        has(&text, "avatar field profile_key name=image_original entries=1");
        has(&text, "avatar field profile_key name=avatar_hash entries=1");
        has(&text, "avatar counted image name=image_original entries=1");
        has(&text, "avatar counted image_host name=image_original host=s3-us-west-2.amazonaws.com urls=1");
        has(&text, "avatar counted marks avatar_hash=1 is_custom_image=1 custom_true=1 of=1");
        // The original's 72-pixel copy on the avatar host is taken.
        has(&text, "avatar counted decoded from=lookup results=1 pictured=1");
        has(&text, &format!("host={} from=lookup field=image_original", slakio_core::images::AVATAR_HOSTS[0]));
        assert!(!text.contains("avatar skipped"), "{text}");
        has(&text, "self-photo lookup result standing=pictured in_results=yes photo=ok");
        has(&text, &format!("self-photo source lookup host={}", slakio_core::images::AVATAR_HOSTS[0]));
    });
}

#[test]
fn the_user_s_own_lookup_tells_the_answer_not_what_a_page_gave_before() {
    rt().block_on(async {
        let mut r = Run::new("lookup-real", None).await;
        r.fake.set_own_photo(false);
        let Event::Booted(b) = r.ask(Command::Boot).await else { panic!("booted") };
        let (me, team) = (b.workspaces[0].me.as_str().to_string(), b.workspaces[0].id.as_str().to_string());
        let photo = format!("https://{}/avatars/me_72.png", slakio_core::images::AVATAR_HOSTS[0]);
        let view = json!({
            "ok": true,
            "history": {"messages": [{"type": "message", "user": me, "text": "mine", "ts": "1767603600.000100"}], "has_more": false},
            "users": [{"id": me, "team_id": team, "profile": {"display_name": "Me", "image_72": photo}}],
        });
        r.fake.fail_next("conversations.view", Fault::Answer(view));
        assert!(matches!(r.ask(Command::History { target: general(), before: None, limit: 20 }).await, Event::History(_)));
        let before = r.text().matches("self-photo lookup result").count();
        assert!(matches!(r.ask(Command::Boot).await, Event::Booted(_)));
        let text = r.text();
        let second: Vec<&str> = text.lines().filter(|l| l.contains("self-photo lookup result")).skip(before).collect();
        // Known with a photo from the page: not asked, so not in the answer.
        assert_eq!(second.len(), 1, "{text}");
        assert!(second[0].ends_with("standing=pictured in_results=no photo=none"), "{text}");
    });
}

#[test]
fn the_user_s_own_record_in_a_view_is_told_once() {
    rt().block_on(async {
        let mut r = Run::new("seen", None).await;
        let Event::Booted(b) = r.ask(Command::Boot).await else { panic!("booted") };
        let (me, team) = (b.workspaces[0].me.as_str().to_string(), b.workspaces[0].id.as_str().to_string());
        let photo = format!("https://{}/avatars/me_72.png", slakio_core::images::AVATAR_HOSTS[0]);
        let view = json!({
            "ok": true,
            "history": {"messages": [{"type": "message", "user": me, "text": "mine", "ts": "1767603600.000100"}], "has_more": false},
            "users": [{"id": me, "team_id": team, "profile": {"display_name": "Me", "image_72": photo}}],
        });
        let world = World::demo();
        let s = world.snapshot();
        let c = s.conversations.iter().find(|c| c.workspace == s.workspaces[0].id && c.name == "general").unwrap();
        let target = slakio_core::model::Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() };
        for _ in 0..2 {
            r.fake.fail_next("conversations.view", Fault::Answer(view.clone()));
            assert!(matches!(r.ask(Command::History { target: target.clone(), before: None, limit: 20 }).await, Event::History(_)));
        }
        let text = r.text();
        has(&text, "self-photo seen ok from=view photo=yes");
        assert_eq!(text.matches("self-photo seen").count(), 1, "once a run: {text}");
    });
}

#[test]
fn an_emoji_list_that_is_not_json_is_told_once() {
    rt().block_on(async {
        let mut r = Run::new("parse", None).await;
        r.fake.fail_next("emoji.list", Fault::Malformed);
        assert!(matches!(r.ask(Command::Emoji).await, Event::Failed { .. }));
        r.fake.fail_next("emoji.list", Fault::Status { status: 503, retry_after: None });
        assert!(matches!(r.ask(Command::Emoji).await, Event::Failed { .. }));
        let text = r.text();
        assert_eq!(text.matches("emoji-list failed parse").count(), 1, "{text}");
        assert_eq!(text.matches("emoji-list failed").count(), 2, "one each: {text}");
        has(&text, "emoji-list failed http");
        has(&text, "emoji-list status 503");
    });
}

#[test]
fn the_sidebar_probe_refused_never_signs_out() {
    rt().block_on(async {
        let mut r = Run::new("probe-auth", None).await;
        r.fake.fail_next("users.channelSections.list", Fault::Api("invalid_auth".to_string()));
        let Event::Booted(b) = r.ask(Command::Boot).await else { panic!("booted") };
        for _ in 0..200 {
            if r.calls("users.channelSections.list") >= 1 && r.text().contains("sections failed") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        has(&r.text(), "sections failed signed-out page=1");
        // Still signed in: the next request goes out and is answered.
        let s = World::demo();
        let snap = s.snapshot();
        let c = snap.conversations.iter().find(|c| c.workspace == b.workspaces[0].id && c.name == "general").unwrap();
        let target =
            slakio_core::model::Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() };
        let page = r.ask(Command::History { target, before: None, limit: 20 }).await;
        assert!(matches!(page, Event::History(_)), "{page:?}");
    });
}

/// The world's `general` conversation.
fn general() -> slakio_core::model::Target {
    let world = World::demo();
    let s = world.snapshot();
    let c = s.conversations.iter().find(|c| c.workspace == s.workspaces[0].id && c.name == "general").unwrap();
    slakio_core::model::Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() }
}

#[test]
fn a_list_asked_before_the_boot_and_slack_s_error_code_are_told() {
    rt().block_on(async {
        let mut r = Run::new("before-boot", None).await;
        r.fake.fail_next("emoji.list", Fault::Api("internal_error".to_string()));
        assert!(matches!(r.ask(Command::Emoji).await, Event::Failed { .. }));
        let text = r.text();
        has(&text, "emoji-list asked ok after_boot=no");
        has(&text, "emoji-list api error code=internal_error");
        has(&text, "emoji-list failed api-internal_error");
    });
}

#[test]
fn a_failed_lookup_and_where_the_own_photo_came_from_are_told() {
    rt().block_on(async {
        let mut r = Run::new("sources", None).await;
        r.fake.set_own_photo(false);
        // The first boot's lookup fails: no photo of the user.
        r.fake.fail_next("users/info", Fault::Status { status: 503, retry_after: None });
        let Event::Booted(b) = r.ask(Command::Boot).await else { panic!("booted") };
        let text = r.text();
        has(&text, "avatar failed http lookup=users-info asked=");
        has(&text, "self-photo source none host=- id=-");
        // A page tells the user's photo; the next boot takes it from there.
        let (me, team) = (b.workspaces[0].me.as_str().to_string(), b.workspaces[0].id.as_str().to_string());
        let photo = format!("https://{}/avatars/me_72.png", slakio_core::images::AVATAR_HOSTS[0]);
        let view = json!({
            "ok": true,
            "history": {"messages": [{"type": "message", "user": me, "text": "mine", "ts": "1767603600.000100"}], "has_more": false},
            "users": [{"id": me, "team_id": team, "profile": {"display_name": "Me", "image_72": photo}}],
        });
        r.fake.fail_next("conversations.view", Fault::Answer(view));
        assert!(matches!(r.ask(Command::History { target: general(), before: None, limit: 20 }).await, Event::History(_)));
        assert!(matches!(r.ask(Command::Boot).await, Event::Booted(_)));
        has(&r.text(), "self-photo source page host=");
    });
}

#[test]
fn a_photo_looked_up_says_so_and_the_picture_share_says_which() {
    rt().block_on(async {
        let limits = Limits { lane_picture_per_minute: 0, ..Limits::default() };
        let cdn = CdnLimits { per_minute: 0, ..CdnLimits::default() };
        let mut r = Run::with_cdn("share", None, limits, cdn).await;
        r.fake.set_own_photo(false);
        let Event::Booted(b) = r.ask(Command::Boot).await else { panic!("booted") };
        has(&r.text(), "self-photo source lookup host=");
        let photo = b.users.iter().find_map(|u| u.avatar.clone()).expect("a photo");
        let image = Command::Image {
            source: slakio_core::images::Source::Avatar(photo),
            max: slakio_core::images::BYTES_MAX,
            list: false,
        };
        assert!(matches!(r.ask(image).await, Event::Failed { .. }));
        let thumb = Command::Image {
            source: slakio_core::images::Source::Thumb(format!("https://{FILE_HOST}/files-tmb/T-F1/shot_360.png")),
            max: slakio_core::images::BYTES_MAX,
            list: false,
        };
        assert!(matches!(r.ask(thumb).await, Event::Failed { .. }));
        let text = r.text();
        for (start, lane) in
            [("avatar cap-reached cdn-budget", "lane=process"), ("thumb cap-reached lane-picture", "lane=workspace")]
        {
            assert!(
                text.lines().any(|l| l.contains(start) && l.contains(lane)),
                "no `{start} … {lane}` line in\n{text}"
            );
        }
        assert!(!text.contains("[secret]"), "{text}");
    });
}

#[test]
fn the_team_and_reaction_probes_log_key_names_only() {
    rt().block_on(async {
        let mut r = Run::new("team-probe", None).await;
        let world = World::demo();
        let ws = &world.snapshot().workspaces[0];
        assert!(matches!(r.ask(Command::Boot).await, Event::Booted(_)));
        // A person of the Slack Connect partner, as a lookup gives them (synthetic).
        let partner = json!({"id": "UPARTNER1", "team_id": "TPARTNER", "name": "partner.person",
            "is_stranger": true, "enterprise_user": {"id": "WPARTNER"},
            "profile": {"display_name": "Partner Person", "team": "TPARTNER", "real_name": "Partner Person"}});
        r.fake.fail_next("users/info", Fault::Answer(json!({"ok": true, "results": [partner]})));
        // A message of theirs with another workspace's emoji (a synthetic Korean name).
        let thanks = "\u{AC10}\u{C0AC}";
        let url = format!("https://{}/TPARTNER/thanks/abc.png", slakio_core::images::EMOJI_HOSTS[0]);
        let message = json!({"type": "message", "ts": "1700000100.000000", "user": "UPARTNER1", "text": "hi",
            "reactions": [{"name": thanks, "count": 4, "users": ["UPARTNER1"], "url": url}],
            "blocks": [{"type": "rich_text", "elements": [{"type": "rich_text_section", "elements": [
                {"type": "emoji", "name": "wave", "url": url}, {"type": "emoji", "name": "smile"}]}]}]});
        r.fake.fail_next("conversations.history", Fault::Answer(json!({"ok": true, "messages": [message]})));
        let before = Some(slakio_core::model::Ts(1_800_000_000_000_000));
        assert!(matches!(r.ask(Command::History { target: general(), before, limit: 28 }).await, Event::History(_)));
        let text = r.text();
        for line in [
            "team-shape field team_key name=id entries=1",
            "team-shape field team_key name=icon entries=1",
            "team-shape field team_icon_key name=image_34 entries=1",
            "team-shape field team_icon_key name=image_default entries=1",
            "team-shape field boot_team_key name=team type=object count=-",
            "team-shape field boot_team_key name=connected_teams type=array count=1",
            "team-shape counted icon_fields where=team name=icon count=1",
            "team-shape counted icon_fields where=team name=image_34 count=1",
            "team-shape counted icon_fields where=connected_teams name=image_44 count=1",
            "team-shape counted external_users count=1",
            "team-shape field external_user_key name=team_id entries=1",
            "team-shape field external_user_key name=is_stranger entries=1",
            "team-shape field external_user_key name=enterprise_user entries=1",
            "team-shape field external_profile_key name=team entries=1",
            "reaction-shape field reaction_key name=url entries=1 page=history",
            "reaction-shape field reaction_key name=name entries=1 page=history",
            "reaction-shape field emoji_element_key name=url entries=1 page=history",
            "reaction-shape field emoji_element_key name=name entries=2 page=history",
            "reaction-shape counted url_on_reaction count=1 page=history",
            "reaction-shape counted url_on_emoji_element count=1 page=history",
        ] {
            has(&text, line);
        }
        for secret in [
            "Partner Co",
            "TPARTNER",
            "WPARTNER",
            "UPARTNER1",
            "Partner Person",
            "partner.person",
            ws.name.unsanitized(),
            ws.id.as_str(),
            "fake",
            "https",
            concat!("slack", "-edge"),
            thanks,
            "thanks",
            "wave",
        ] {
            let lines: Vec<&str> =
                text.lines().filter(|l| l.contains("team-shape") || l.contains("reaction-shape")).collect();
            assert!(lines.iter().all(|l| !l.contains(secret)), "{secret:?} in\n{}", lines.join("\n"));
        }
        assert!(text.lines().all(|l| !l.contains("Partner Co") && !l.contains("Partner Person")), "{text}");
    });
}

/// The page probes are told once per page kind, however many pages come; a lookup that finds
/// none of another workspace says so once.
#[test]
fn two_history_pages_give_the_page_probes_once() {
    rt().block_on(async {
        let mut r = Run::new("probes-once", None).await;
        assert!(matches!(r.ask(Command::Boot).await, Event::Booted(_)));
        let team = World::demo().snapshot().workspaces[0].id.as_str().to_string();
        let mine = json!({"id": "UMINE1", "team_id": team, "name": "mine", "profile": {"display_name": "Mine"}});
        for k in 0..2 {
            r.fake.fail_next("users/info", Fault::Answer(json!({"ok": true, "results": [mine.clone()]})));
            let message = json!({"type": "message", "ts": format!("170000010{k}.000000"), "user": "UMINE1",
                "text": "hi", "reactions": [{"name": "wave", "count": 1, "users": ["UMINE1"]}]});
            r.fake.fail_next("conversations.history", Fault::Answer(json!({"ok": true, "messages": [message]})));
            let before = Some(slakio_core::model::Ts(1_800_000_000_000_000 - k));
            let page = r.ask(Command::History { target: general(), before, limit: 28 }).await;
            assert!(matches!(page, Event::History(_)));
        }
        let text = r.text();
        for line in [
            "reaction-shape counted url_on_reaction count=0 page=history",
            "reaction-shape field reaction_key name=name",
        ] {
            assert_eq!(text.matches(line).count(), 1, "`{line}` once in\n{text}");
        }
        assert!(text.matches("external_users count=0").count() <= 1, "none of another workspace told once:\n{text}");
    });
}

/// Every view page tells the people of another workspace it brings; its reactions only the
/// first view page of the run.
#[test]
fn every_view_page_gives_the_people_probe_and_the_first_the_reactions() {
    rt().block_on(async {
        let mut r = Run::new("probes-view", None).await;
        let Event::Booted(b) = r.ask(Command::Boot).await else { panic!("booted") };
        let me = b.workspaces[0].me.as_str().to_string();
        let view = json!({
            "ok": true,
            "history": {"messages": [{"type": "message", "user": me, "text": "hi", "ts": "1767603600.000100",
                "reactions": [{"name": "wave", "count": 1, "users": [me]}]}], "has_more": false},
            "users": [{"id": "UPARTNER1", "team_id": "TPARTNER", "name": "partner.person",
                "profile": {"display_name": "Partner Person", "team": "TPARTNER"}}],
        });
        for _ in 0..2 {
            r.fake.fail_next("conversations.view", Fault::Answer(view.clone()));
            assert!(matches!(
                r.ask(Command::History { target: general(), before: None, limit: 20 }).await,
                Event::History(_)
            ));
        }
        let text = r.text();
        for line in ["team-shape counted external_users count=1", "team-shape field external_user_key name=team_id"] {
            assert_eq!(text.matches(line).count(), 2, "`{line}` for each view page in\n{text}");
        }
        for line in
            ["reaction-shape counted url_on_reaction count=0 page=view", "reaction-shape field reaction_key name=name"]
        {
            assert_eq!(text.matches(line).count(), 1, "`{line}` once in\n{text}");
        }
    });
}
