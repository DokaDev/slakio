//! The sidebar's sections never hold the boot answer back: when Slack is slow to list them, the
//! answer comes with the default sections, and Slack's follow as an update once they come.

use slakio_core::backend::{BackendHandle, Command, Envelope, Event, Generation, Live, Request};
use slakio_core::model::SectionKind;
use slakio_core::secret::Credentials;
use slakio_fake::{FakeSlack, Fault};
use slakio_slack::Client;
use slakio_slack::governor::{FakeClock, Governor, Limits};
use slakio_slack::request::Endpoints;
use slakio_world::World;
use std::time::{Duration, Instant};

const SECTIONS: &str = "users.channelSections.list";

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

async fn next(handle: &mut BackendHandle, wait: Duration) -> Option<Envelope> {
    tokio::time::timeout(wait, handle.rx.recv()).await.ok().flatten()
}

#[test]
fn a_slow_sections_answer_does_not_hold_the_boot_and_follows_as_an_update() {
    rt().block_on(async {
        let fake = FakeSlack::start(World::new(7)).await.unwrap();
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let client = Client::new(Credentials::new("t", "c"), Endpoints::local(fake.addr()).unwrap(), governor);
        fake.fail_next(SECTIONS, Fault::Late(Duration::from_secs(6)));
        let mut handle = slakio_slack::serve(client);
        let asked = Instant::now();
        handle.send(Request { id: Generation(1), workspace: None, command: Command::Boot }).unwrap();
        let booted = next(&mut handle, Duration::from_secs(20)).await.expect("the boot answer");
        assert!(asked.elapsed() < Duration::from_secs(4), "the boot waited for the sections: {:?}", asked.elapsed());
        assert_eq!(booted.generation, Generation(1));
        let Event::Booted(s) = booted.event else { panic!("{booted:?}") };
        assert!(s.sections_unknown, "the sections are not known yet");
        let kinds: Vec<SectionKind> = s.sections.iter().map(|x| x.kind).collect();
        assert_eq!(kinds, [SectionKind::Channels, SectionKind::DirectMessages], "the default sections");
        // Slack's sections follow, pushed: every conversation placed in one of them.
        let update = next(&mut handle, Duration::from_secs(20)).await.expect("the sections update");
        assert_eq!(update.generation, Generation::PUSHED);
        let Event::Live(live) = update.event else { panic!("{update:?}") };
        let Live::Sections { workspace, sections, places } = *live else { panic!("{live:?}") };
        assert_eq!(workspace, s.workspaces[0].id);
        assert!(sections.iter().any(|x| x.kind == SectionKind::Favorites), "Slack's Starred: {sections:?}");
        assert!(sections.iter().any(|x| x.kind == SectionKind::Custom), "a section of the user's own");
        assert_eq!(places.len(), s.conversations.len());
        assert!(places.iter().all(|(_, at)| sections.iter().any(|x| &x.id == at)), "each in a section listed");
    });
}

#[test]
fn sections_that_come_in_time_are_in_the_boot_answer_and_no_update_follows() {
    rt().block_on(async {
        let fake = FakeSlack::start(World::new(7)).await.unwrap();
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let client = Client::new(Credentials::new("t", "c"), Endpoints::local(fake.addr()).unwrap(), governor);
        let mut handle = slakio_slack::serve(client);
        handle.send(Request { id: Generation(1), workspace: None, command: Command::Boot }).unwrap();
        let booted = next(&mut handle, Duration::from_secs(20)).await.expect("the boot answer");
        let Event::Booted(s) = booted.event else { panic!("{booted:?}") };
        assert!(!s.sections_unknown);
        assert!(s.sections.iter().any(|x| x.kind == SectionKind::Favorites));
        assert!(next(&mut handle, Duration::from_millis(300)).await.is_none(), "nothing follows");
    });
}

#[test]
fn sections_slack_refuses_are_unknown_and_no_update_follows() {
    rt().block_on(async {
        let fake = FakeSlack::start(World::new(7)).await.unwrap();
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let client = Client::new(Credentials::new("t", "c"), Endpoints::local(fake.addr()).unwrap(), governor);
        fake.fail_next(SECTIONS, Fault::Status { status: 500, retry_after: None });
        let mut handle = slakio_slack::serve(client);
        handle.send(Request { id: Generation(1), workspace: None, command: Command::Boot }).unwrap();
        let booted = next(&mut handle, Duration::from_secs(20)).await.expect("the boot answer");
        let Event::Booted(s) = booted.event else { panic!("{booted:?}") };
        assert!(s.sections_unknown, "unknown, so the sections known before stay");
        assert!(next(&mut handle, Duration::from_millis(300)).await.is_none(), "nothing follows");
    });
}

/// A boot whose sections come late, booted again meanwhile: what the older boot reads comes
/// after the newer placed the sidebar, so it is let go (the newer's sections, refused here, stay
/// unknown and nothing follows).
#[test]
fn sections_an_older_boot_reads_late_are_let_go_after_a_newer_boot() {
    rt().block_on(async {
        let fake = FakeSlack::start(World::new(7)).await.unwrap();
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let client = Client::new(Credentials::new("t", "c"), Endpoints::local(fake.addr()).unwrap(), governor);
        let late = Duration::from_secs(5);
        fake.fail_next(SECTIONS, Fault::Late(late));
        fake.fail_next(SECTIONS, Fault::Status { status: 500, retry_after: None });
        let mut handle = slakio_slack::serve(client);
        let asked = Instant::now();
        for id in [1, 2] {
            handle.send(Request { id: Generation(id), workspace: None, command: Command::Boot }).unwrap();
            let booted = next(&mut handle, Duration::from_secs(20)).await.expect("the boot answer");
            assert_eq!(booted.generation, Generation(id));
            let Event::Booted(s) = booted.event else { panic!("{booted:?}") };
            assert!(s.sections_unknown, "boot {id}: the sections are not known yet");
        }
        // Past the older boot's late answer, and then some: nothing came.
        let wait = (late + Duration::from_secs(3)).saturating_sub(asked.elapsed());
        assert!(next(&mut handle, wait).await.is_none(), "the older boot's sections are let go");
        assert_eq!(fake.methods().iter().filter(|m| *m == SECTIONS).count(), 2, "both boots asked");
    });
}
