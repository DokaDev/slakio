use super::*;
use crate::governor::{FakeClock, Governor, Limits};
use crate::request::Endpoints;
use slakio_core::secret::Credentials;
use slakio_fake::FakeSlack;
use slakio_world::World;

#[test]
fn many_ids_go_in_batches_of_eighty_and_unknown_ones_stay_unnamed() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let fake = FakeSlack::start(World::new(7)).await.unwrap();
        let world = World::new(7);
        let team = world.snapshot().workspaces[0].id.as_str().to_string();
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let client = Client::new(Credentials::new("t", "c"), Endpoints::local(fake.addr()).unwrap(), governor);
        client.set_team(&team);
        let people = People::default();
        people.set_team(&team);
        let real: Vec<String> = world.snapshot().users.iter().take(10).map(|u| u.id.as_str().to_string()).collect();
        let mut ids: Vec<String> = (0..190).map(|i| format!("UNKNOWN{i:03}")).collect();
        ids.extend(real.iter().cloned());
        ids.extend(real.iter().cloned());
        let named = people.names(&client, &ids, Priority::User).await;
        assert_eq!(named.len(), real.len(), "only the ids Slack knows get a name");
        let sizes: Vec<usize> = fake
            .log()
            .iter()
            .map(|l| {
                serde_json::from_str::<serde_json::Value>(&l.body).unwrap()["updated_ids"].as_object().unwrap().len()
            })
            .collect();
        assert_eq!(sizes, [80, 80, 40], "200 distinct ids, 80 a request");
        // Known ones are not asked again; those Slack has no answer for, at most the cap.
        let _ = people.names(&client, &real, Priority::User).await;
        assert_eq!(fake.log().len(), 3);
        for _ in 0..4 {
            let _ = people.names(&client, &ids, Priority::User).await;
        }
        assert_eq!(fake.log().len(), 3 * usize::from(OMITTED_MAX), "190 ids, 80 a request, each at most 3 times");
    });
}

#[test]
fn an_id_whose_lookup_failed_is_asked_again() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let fake = FakeSlack::start(World::new(7)).await.unwrap();
        let team = World::new(7).snapshot().workspaces[0].id.as_str().to_string();
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let client = Client::new(Credentials::new("t", "c"), Endpoints::local(fake.addr()).unwrap(), governor);
        client.set_team(&team);
        let people = People::default();
        people.set_team(&team);
        fake.fail_next("users/info", slakio_fake::Fault::Status { status: 503, retry_after: None });
        let ids = ["UNKNOWN001".to_string()];
        for _ in 0..5 {
            assert!(people.names(&client, &ids, Priority::User).await.is_empty());
        }
        assert_eq!(fake.log().len(), 1 + usize::from(OMITTED_MAX), "asked again after the failure, up to the cap");
    });
}

fn person(id: &str, photo: Option<&str>) -> wire::Person {
    let mut profile = json!({"display_name": "Minsu"});
    if let Some(p) = photo {
        profile["image_72"] = json!(p);
    }
    serde_json::from_value(json!({"id": id, "team_id": "T1", "profile": profile})).unwrap()
}

fn photo_of(people: &People, id: &str) -> Option<String> {
    people.known.lock().unwrap().get(id).and_then(|u| u.avatar.clone())
}

#[test]
fn a_record_without_a_photo_keeps_the_photo_known_and_a_new_photo_replaces_it() {
    let people = People::default();
    people.set_team("T1");
    let host = slakio_core::images::AVATAR_HOSTS[0];
    let (one, two) = (format!("https://{host}/one_72.png"), format!("https://{host}/two_72.png"));
    let (one, two) = (one.as_str(), two.as_str());
    people.learn(&[person("U1", Some(one))]);
    people.learn(&[person("U1", None)]);
    assert_eq!(photo_of(&people, "U1").as_deref(), Some(one), "no photo in a later record is not no photo");
    people.learn(&[person("U1", Some("https://elsewhere.example/x.png"))]);
    assert_eq!(photo_of(&people, "U1").as_deref(), Some(one), "a photo on another host is no photo");
    people.learn(&[person("U1", Some(two))]);
    assert_eq!(photo_of(&people, "U1").as_deref(), Some(two), "a real new photo replaces it");
}

#[test]
fn a_lookup_with_only_the_original_keeps_a_given_image_72() {
    let people = People::default();
    people.set_team("T1");
    let host = slakio_core::images::AVATAR_HOSTS[0];
    let given = format!("https://{host}/2026-01-05/real-name_72.jpg");
    let original = format!("https://{host}/2026-01-05/T1-U1_original.png");
    let guess = format!("https://{host}/2026-01-05/T1-U1_72.png");
    let only_original = |id: &str| -> wire::Person {
        serde_json::from_value(json!({"id": id, "team_id": "T1",
            "profile": {"display_name": "Minsu", "image_original": original}}))
        .unwrap()
    };
    people.learn(&[person("U1", Some(&given))]);
    people.learn(&[only_original("U1")]);
    assert_eq!(photo_of(&people, "U1").as_deref(), Some(given.as_str()), "a guess never replaces a given photo");
    assert!(!people.known.lock().unwrap()["U1"].avatar_guessed);
    // Known only by a guess: the guess is kept, and a given photo replaces it.
    people.learn(&[only_original("U2")]);
    assert_eq!(photo_of(&people, "U2").as_deref(), Some(guess.as_str()));
    assert!(people.known.lock().unwrap()["U2"].avatar_guessed);
    people.learn(&[person("U2", Some(&given))]);
    assert_eq!(photo_of(&people, "U2").as_deref(), Some(given.as_str()));
    assert!(!people.known.lock().unwrap()["U2"].avatar_guessed);
}

#[test]
fn a_record_saying_no_photo_of_their_own_drops_the_photo_known() {
    let people = People::default();
    people.set_team("T1");
    let one = format!("https://{}/one_72.png", slakio_core::images::AVATAR_HOSTS[0]);
    people.learn(&[person("U1", Some(&one))]);
    // Slack's default avatar only, as a later lookup says it: the photo known before is gone.
    let default = serde_json::from_value(json!({"id": "U1", "team_id": "T1",
        "profile": {"display_name": "Minsu", "image_72": one, "is_custom_image": false}}))
    .unwrap();
    people.learn(&[default]);
    assert_eq!(photo_of(&people, "U1"), None, "no photo of their own: the old one is gone");
    assert!(people.known.lock().unwrap()["U1"].no_photo);
    // A later record that says nothing of a photo leaves none: nothing comes back.
    people.learn(&[person("U1", None)]);
    assert_eq!(photo_of(&people, "U1"), None);
}

#[test]
fn a_profile_change_is_kept_for_a_person_known_and_given_back_for_anyone() {
    let people = People::default();
    people.set_team("T1");
    let host = slakio_core::images::AVATAR_HOSTS[0];
    let (one, two) = (format!("https://{host}/one_72.png"), format!("https://{host}/two_72.png"));
    people.learn(&[person("U1", Some(&one))]);
    let u = people.changed(&person("U1", Some(&two))).unwrap();
    assert_eq!(u.avatar.as_deref(), Some(two.as_str()));
    assert_eq!(photo_of(&people, "U1").as_deref(), Some(two.as_str()), "kept: a later page shows it");
    let u = people.changed(&person("U9", Some(&one))).unwrap();
    assert_eq!(u.id.as_str(), "U9");
    assert!(!people.known.lock().unwrap().contains_key("U9"), "one not known is not kept");
}

#[test]
fn a_profile_change_keeps_whether_one_known_is_a_bot_and_of_another_organization() {
    let people = People::default();
    people.set_team("T1");
    let bot = serde_json::from_value(json!({"id": "B1", "team_id": "T2", "is_bot": true,
        "profile": {"display_name": "Deploys"}}))
    .unwrap();
    people.learn(&[bot]);
    // The change's record says neither: not a bot, nor of another organization, as it reads.
    let u = people.changed(&person("B1", None)).unwrap();
    assert_eq!(u.display_name, "Minsu");
    let kept = people.known.lock().unwrap()["B1"].clone();
    for u in [u, kept] {
        assert!(u.bot);
        assert_eq!(u.org, Org::External(Remote::new("T2")));
    }
}

#[test]
fn only_a_person_known_without_a_photo_is_forgotten_to_be_asked_again() {
    let people = People::default();
    people.set_team("T1");
    let two = format!("https://{}/two_72.png", slakio_core::images::AVATAR_HOSTS[0]);
    people.learn(&[person("U1", None), person("U2", Some(&two))]);
    people.forget_unpictured("U1");
    people.forget_unpictured("U2");
    let known = people.known.lock().unwrap();
    assert!(!known.contains_key("U1"));
    assert!(known.contains_key("U2"));
}

/// A fake Slack, a client and the people of its team; the fake's first edge lookup answers `first`.
async fn looking(first: serde_json::Value) -> (FakeSlack, Client, People) {
    let fake = FakeSlack::start(World::new(7)).await.unwrap();
    let team = World::new(7).snapshot().workspaces[0].id.as_str().to_string();
    let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
    let client = Client::new(Credentials::new("t", "c"), Endpoints::local(fake.addr()).unwrap(), governor);
    client.set_team(&team);
    let people = People::default();
    people.set_team(&team);
    fake.fail_next("users/info", slakio_fake::Fault::Answer(first));
    (fake, client, people)
}

fn asked_for(fake: &FakeSlack, id: &str) -> usize {
    fake.log().iter().filter(|l| l.method() == "users/info" && l.body.contains(id)).count()
}

#[test]
fn a_person_forgotten_for_their_photo_is_asked_again_though_once_unnamed() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let me = World::new(7).snapshot().workspaces[0].me.as_str().to_string();
        // The first lookup names no one: Slack answered for the user without a name.
        let (fake, client, people) = looking(json!({"ok": true, "results": [{"id": me}]})).await;
        let ids = [me.clone()];
        assert!(people.names(&client, &ids, Priority::User).await.is_empty());
        assert_eq!(people.standing(&me), "unnamed");
        people.forget_unpictured(&me);
        assert_eq!(people.standing(&me), "unknown");
        let named = people.names(&client, &ids, Priority::User).await;
        assert_eq!(asked_for(&fake, &me), 2, "asked again after it was forgotten");
        assert_eq!(named.len(), 1);
    });
}

#[test]
fn an_id_left_out_or_pending_is_asked_again_and_a_failed_one_is_not() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let me = World::new(7).snapshot().workspaces[0].me.as_str().to_string();
        let (fake, client, people) = looking(json!({"ok": true, "results": [], "pending_ids": [me]})).await;
        let ids = [me.clone()];
        assert!(people.names(&client, &ids, Priority::User).await.is_empty());
        assert_eq!(people.standing(&me), "unknown", "pending is not unnamed");
        assert_eq!(people.names(&client, &ids, Priority::User).await.len(), 1, "asked again, named");
        assert_eq!(asked_for(&fake, &me), 2);
        // Left out with no word: asked again, up to the cap.
        fake.fail_next("users/info", slakio_fake::Fault::Answer(json!({"ok": true, "results": []})));
        fake.fail_next("users/info", slakio_fake::Fault::Answer(json!({"ok": true, "results": []})));
        fake.fail_next("users/info", slakio_fake::Fault::Answer(json!({"ok": true, "results": []})));
        let other = ["UOMITTED".to_string()];
        for _ in 0..5 {
            let _ = people.names(&client, &other, Priority::User).await;
        }
        assert_eq!(asked_for(&fake, "UOMITTED"), usize::from(OMITTED_MAX), "at most the cap");
        // Listed as failed (which may pass): asked again like a pending one, up to the cap.
        let gone = ["UGONE".to_string()];
        for _ in 0..3 {
            fake.fail_next(
                "users/info",
                slakio_fake::Fault::Answer(json!({"ok": true, "results": [], "failed_ids": ["UGONE"]})),
            );
        }
        for _ in 0..5 {
            let _ = people.names(&client, &gone, Priority::User).await;
        }
        assert_eq!(asked_for(&fake, "UGONE"), usize::from(OMITTED_MAX));
    });
}

#[test]
fn an_id_listed_as_failed_twice_is_named_by_the_third_lookup() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let me = World::new(7).snapshot().workspaces[0].me.as_str().to_string();
        let failed = json!({"ok": true, "results": [], "failed_ids": [me]});
        let (fake, client, people) = looking(failed.clone()).await;
        fake.fail_next("users/info", slakio_fake::Fault::Answer(failed));
        let ids = [me.clone()];
        assert!(people.names(&client, &ids, Priority::User).await.is_empty());
        assert!(people.names(&client, &ids, Priority::User).await.is_empty());
        assert_eq!(people.standing(&me), "unknown", "failed twice is not unnamed");
        assert_eq!(people.names(&client, &ids, Priority::User).await.len(), 1, "named on the third");
        assert_eq!(asked_for(&fake, &me), 3);
    });
}

#[test]
fn only_the_ids_asked_for_are_taken_and_a_named_one_never_becomes_unnamed() {
    let people = People::default();
    people.set_team("T1");
    let asked = "UASKED".to_string();
    let named = |id: &str| json!({"id": id, "team_id": "T1", "profile": {"display_name": "Minsu"}});
    let found: wire::EdgeUsers = serde_json::from_value(json!({
        "ok": true,
        "results": [named("UASKED"), {"id": "UASKED"}, named("UOTHER")],
    }))
    .unwrap();
    assert_eq!(people.take("T1", &[&asked], found, None), None);
    assert_eq!(people.standing("UASKED"), "unpictured");
    assert!(!people.unnamed.lock().unwrap().contains("UASKED"), "its nameless duplicate is not unnamed");
    assert_eq!(people.standing("UOTHER"), "unknown", "not asked for: not taken");
}

/// Whether a lookup's answer had the person watched is that lookup's own (nothing is kept for
/// anyone else): a person not asked, or not watched, says nothing.
#[test]
fn a_lookup_tells_only_of_the_person_it_watches_and_keeps_nothing_of_it() {
    let people = People::default();
    people.set_team("T1");
    let (one, two) = ("UONE".to_string(), "UTWO".to_string());
    let photo = format!("https://{}/one_72.png", slakio_core::images::AVATAR_HOSTS[0]);
    let found = || -> wire::EdgeUsers {
        serde_json::from_value(json!({"ok": true, "results": [
            {"id": "UONE", "team_id": "T1", "profile": {"display_name": "One", "image_72": photo}},
            {"id": "UTWO", "team_id": "T1", "profile": {"display_name": "Two"}},
        ]}))
        .unwrap()
    };
    assert_eq!(people.take("T1", &[&one, &two], found(), Some("UONE")), Some(true));
    assert_eq!(people.take("T1", &[&one, &two], found(), Some("UTWO")), Some(false));
    assert_eq!(people.take("T1", &[&one], found(), Some("UTWO")), None, "not asked for");
    assert_eq!(people.take("T1", &[&one, &two], found(), None), None);
}

#[test]
fn an_earlier_lookup_of_the_person_watched_says_nothing_of_a_later_one() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let fake = FakeSlack::start(World::new(7)).await.unwrap();
        let world = World::new(7);
        let team = world.snapshot().workspaces[0].id.as_str().to_string();
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let client = Client::new(Credentials::new("t", "c"), Endpoints::local(fake.addr()).unwrap(), governor);
        client.set_team(&team);
        let people = People::default();
        people.set_team(&team);
        let id = world.snapshot().users[1].id.as_str().to_string();
        let ids = [id.clone()];
        // Another lookup had them; the later one, which watches them, finds them known and asks
        // nothing: it says nothing of them.
        let _ = people.names(&client, &ids, Priority::User).await;
        let (named, answer) = people.lookup(&client, &ids, Priority::User, Some(&id)).await;
        assert_eq!((named.len(), answer), (1, None));
    });
}

/// The JSON body of each edge lookup `fake` was sent.
fn lookup_bodies(fake: &FakeSlack) -> Vec<serde_json::Value> {
    let log = fake.log().into_iter().filter(|l| l.method() == "users/info");
    log.map(|l| serde_json::from_str(&l.body).unwrap()).collect()
}

#[test]
fn a_lookup_asks_each_id_at_version_zero_for_the_whole_profile_and_keeps_the_photos() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let world = World::new(7);
        let world = world.snapshot();
        let team = world.workspaces[0].id.as_str().to_string();
        let fake = FakeSlack::start(World::new(7)).await.unwrap();
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let client = Client::new(Credentials::new("t", "c"), Endpoints::local(fake.addr()).unwrap(), governor);
        client.set_team(&team);
        let people = People::default();
        people.set_team(&team);
        let users: Vec<&User> = world.users.iter().filter(|u| u.workspace.as_str() == team).take(12).collect();
        let ids: Vec<String> = users.iter().map(|u| u.id.as_str().to_string()).collect();
        let named = people.names(&client, &ids, Priority::User).await;
        let versions: Map<String, Value> = ids.iter().map(|id| (id.clone(), json!(0))).collect();
        assert_eq!(
            lookup_bodies(&fake),
            [json!({"check_interaction": true, "token": "t", "updated_ids": versions})],
            "every id at version 0, and the profile asked for whether or not it is stale"
        );
        assert_eq!(named.len(), ids.len());
        let pictured = users.iter().filter(|u| u.avatar.is_some() && !u.id.as_str().ends_with("666")).count();
        assert!(pictured > 0 && pictured < users.len(), "some with a photo, some without");
        for u in &users {
            let got = named.iter().find(|n| n.id == u.id).unwrap();
            let want = u.avatar.clone().filter(|_| !u.id.as_str().ends_with("666"));
            assert_eq!(got.avatar, want, "{}: the photo the lookup answered", u.id.as_str());
            assert_eq!(people.standing(u.id.as_str()), if want.is_some() { "pictured" } else { "unpictured" });
        }
    });
}

#[test]
fn the_fake_edge_cache_leaves_photos_out_only_when_asked_for_stale_profiles_only() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let world = World::new(7);
        let world = world.snapshot();
        let team = world.workspaces[0].id.as_str().to_string();
        let fake = FakeSlack::start(World::new(7)).await.unwrap();
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let client = Client::new(Credentials::new("t", "c"), Endpoints::local(fake.addr()).unwrap(), governor);
        client.set_team(&team);
        let pictured: Vec<&User> = world.users.iter().filter(|u| u.avatar.is_some()).take(5).collect();
        assert!(!pictured.is_empty());
        let versions: Map<String, Value> = pictured.iter().map(|u| (u.id.as_str().to_string(), json!(0))).collect();
        let mut photos = Vec::new();
        for stale_only in [true, false] {
            let mut body = Map::new();
            body.insert("check_interaction".into(), json!(true));
            if stale_only {
                body.insert("include_profile_only_if_stale".into(), json!(true));
            }
            body.insert("updated_ids".into(), Value::Object(versions.clone()));
            let call = Call::json(Method::EdgeUsersInfo, body);
            let found = client.call::<wire::EdgeUsers>(&call, Priority::User).await.unwrap();
            let results = found.results.unwrap();
            assert_eq!(results.len(), pictured.len(), "every person answered, with a name");
            assert!(results.iter().all(|p| p.shown_name().is_some()));
            photos.push(results.iter().filter_map(|p| p.profile.as_ref().and_then(wire::Profile::avatar)).count());
        }
        assert_eq!(photos, [0, pictured.len()], "stale profiles only: no photo; the whole profile: each photo");
    });
}

#[test]
fn a_looked_up_record_with_a_photo_is_pictured_and_one_without_is_named_unpictured() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let team = World::new(7).snapshot().workspaces[0].id.as_str().to_string();
        let photo = format!("https://{}/photo_72.png", slakio_core::images::AVATAR_HOSTS[0]);
        let answer = json!({"ok": true, "results": [
            {"id": "UPHOTO", "team_id": team, "profile": {"display_name": "Minsu", "image_72": photo}},
            {"id": "UPLAIN", "team_id": team, "profile": {"display_name": "Jiwoo"}},
        ]});
        let (fake, client, people) = looking(answer).await;
        let ids = ["UPHOTO".to_string(), "UPLAIN".to_string()];
        let named = people.names(&client, &ids, Priority::User).await;
        let versions = json!({"UPHOTO": 0, "UPLAIN": 0});
        assert_eq!(lookup_bodies(&fake), [json!({"check_interaction": true, "token": "t", "updated_ids": versions})]);
        assert_eq!(named.len(), 2);
        assert_eq!(photo_of(&people, "UPHOTO").as_deref(), Some(photo.as_str()));
        assert_eq!(people.standing("UPHOTO"), "pictured");
        assert_eq!(photo_of(&people, "UPLAIN"), None, "no photo in the answer: none known");
        assert_eq!(people.standing("UPLAIN"), "unpictured", "named all the same");
    });
}
