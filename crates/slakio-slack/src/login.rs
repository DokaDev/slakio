//! Whether credentials sign in: one `client.userBoot`, the call the web client starts with, and
//! nothing else. Its answer names the workspace and the user.

use crate::client::Client;
use crate::error::SlackError;
use crate::governor::Priority;
use crate::method::Method;
use crate::request::Call;
use crate::wire;

/// Who the credentials sign in as. The names are Slack's text (remote: shown through the
/// sanitiser).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedIn {
    pub team: String,
    pub team_name: Option<String>,
    pub user: String,
    pub user_name: Option<String>,
}

/// `client.userBoot` as the web client asks for it.
pub(crate) fn user_boot() -> Call {
    Call::form(
        Method::ClientUserBoot,
        &[
            ("version_all_channels", "false"),
            ("return_all_relevant_mpdms", "true"),
            ("omit_extras", "feature_usage_data,plan_info,salesforce_features"),
        ],
    )
}

/// Ask Slack, with one `client.userBoot`, whom `client`'s credentials sign in as.
pub async fn check(client: &Client) -> Result<SignedIn, SlackError> {
    let boot: wire::UserBoot = client.call(&user_boot(), Priority::User).await?;
    match (boot.team, boot.me) {
        (Some(team), Some(me)) => Ok(SignedIn { team: team.id, team_name: team.name, user: me.id, user_name: me.name }),
        _ => Err(SlackError::Network { detail: "the start answer has no team or user".to_string() }),
    }
}
