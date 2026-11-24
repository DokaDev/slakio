//! Only the diagnostics' probe of an answer's shape is exempt from the sign-out a refused call
//! means: a call of the same method without the flag, or any call after the probe's refusal,
//! still signs the workspace out.

use slakio_core::secret::Credentials;
use slakio_fake::{FakeSlack, Fault};
use slakio_slack::Client;
use slakio_slack::error::SlackError;
use slakio_slack::governor::{FakeClock, Governor, Limits, Priority};
use slakio_slack::method::Method;
use slakio_slack::request::{Call, Endpoints};
use slakio_world::World;

const SECTIONS: &str = "users.channelSections.list";

async fn client() -> (FakeSlack, Client) {
    let fake = FakeSlack::start(World::new(7)).await.unwrap();
    let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
    let client = Client::new(Credentials::new("t", "c"), Endpoints::local(fake.addr()).unwrap(), governor);
    client.set_team(World::new(7).snapshot().workspaces[0].id.as_str());
    (fake, client)
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

/// Signed out: the next call fails at once, with nothing sent.
async fn signed_out(fake: &FakeSlack, client: &Client) -> bool {
    let before = fake.log().len();
    let next = client.call::<serde_json::Value>(&Call::form(Method::ClientCounts, &[]), Priority::User).await;
    matches!(next, Err(SlackError::Auth { .. })) && fake.log().len() == before
}

#[test]
fn the_method_of_the_probe_without_its_flag_still_signs_out() {
    rt().block_on(async {
        let (fake, client) = client().await;
        fake.fail_next(SECTIONS, Fault::Api("invalid_auth".to_string()));
        let call = Call::form(Method::UsersChannelSectionsList, &[]);
        assert!(client.call::<serde_json::Value>(&call, Priority::Background).await.is_err());
        assert!(signed_out(&fake, &client).await, "a refusal of the same method is no probe's");
    });
}

#[test]
fn after_the_probe_s_refusal_a_refused_call_still_signs_out() {
    rt().block_on(async {
        let (fake, client) = client().await;
        fake.fail_next(SECTIONS, Fault::Api("invalid_auth".to_string()));
        let probe = Call::form(Method::UsersChannelSectionsList, &[]).probe();
        assert!(client.call::<serde_json::Value>(&probe, Priority::Background).await.is_err());
        assert!(!signed_out(&fake, &client).await, "the probe's refusal keeps the sign-in");
        fake.fail_next("client.counts", Fault::Api("invalid_auth".to_string()));
        assert!(
            client.call::<serde_json::Value>(&Call::form(Method::ClientCounts, &[]), Priority::User).await.is_err()
        );
        assert!(signed_out(&fake, &client).await);
    });
}
