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
        // Known ones are not asked again, nor those Slack answered for without a name.
        let _ = people.names(&client, &real, Priority::User).await;
        let _ = people.names(&client, &ids, Priority::User).await;
        assert_eq!(fake.log().len(), 3);
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
        assert!(people.names(&client, &ids, Priority::User).await.is_empty());
        assert!(people.names(&client, &ids, Priority::User).await.is_empty());
        let _ = people.names(&client, &ids, Priority::User).await;
        assert_eq!(fake.log().len(), 2, "asked again after the failure, then known as unnamed");
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
