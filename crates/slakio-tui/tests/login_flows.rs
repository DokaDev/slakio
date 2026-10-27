//! `slakio login` and `slakio logout` as a user runs them, against the tests' fake Slack (a
//! debug build signs in to it instead of a workspace's hosts): the paste piped in, one call to
//! check it, nothing secret printed or written outside the (in-memory) secret store.

use slakio_fake::{FakeSlack, Fault};
use slakio_world::World;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// Synthetic credentials (spelled in parts so no file holds a token-shaped string).
const TOKEN: &str = concat!("xo", "xc-1-2-TESTTOKEN");
const COOKIE: &str = concat!("xo", "xd-TESTCOOKIE%2F1");

fn paste() -> String {
    format!(
        "curl 'https://example.slack.com/api/client.counts?fp=6e' \\\n  -H 'cookie: b=1; d={COOKIE}; d-s=17' \\\n  --data-raw 'token={TOKEN}'"
    )
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slakio-login-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run the binary with `args` and `stdin`, every directory inside `dir`, credentials in memory,
/// signing in to the fake Slack at `fake` when given.
fn slakio(dir: &Path, fake: Option<String>, args: &[&str], stdin: &str) -> Output {
    use std::io::Write;
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_slakio"));
    cmd.args(args)
        .env("SLAKIO_SECRET_STORE", "memory")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("LC_ALL", "en_US.UTF-8")
        .env_remove("SLAKIO_NETWORK")
        .env_remove("SLAKIO_TEST_SLACK_ADDR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(addr) = fake {
        cmd.env("SLAKIO_TEST_SLACK_ADDR", addr);
    }
    let mut child = cmd.spawn().unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

/// `slakio <args>` against a fake Slack set up by `faults`: its output, and the methods the
/// fake was asked for.
fn against_fake(dir: &Path, faults: &[(&str, Fault)], args: &[&str], stdin: &str) -> (Output, Vec<String>) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let fake = FakeSlack::start(World::new(7)).await.unwrap();
        for (method, fault) in faults {
            fake.fail_next(method, fault.clone());
        }
        let (dir, addr) = (dir.to_path_buf(), fake.addr().to_string());
        let (args, stdin): (Vec<String>, String) = (args.iter().map(ToString::to_string).collect(), stdin.to_string());
        let out = tokio::task::spawn_blocking(move || {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            slakio(&dir, Some(addr), &args, &stdin)
        })
        .await
        .unwrap();
        (out, fake.methods())
    })
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// Neither the output nor any file the binary wrote holds the token or the cookie.
fn nothing_secret(dir: &Path, said: &str) {
    assert!(!said.contains(TOKEN) && !said.contains(COOKIE), "{said}");
    for file in walk(dir) {
        let body = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(!body.contains(TOKEN) && !body.contains(COOKIE), "{}: {body}", file.display());
    }
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn workspaces(dir: &Path) -> PathBuf {
    dir.join("config").join("slakio").join("workspaces.toml")
}

#[test]
fn login_checks_the_paste_once_keeps_it_and_names_the_workspace_then_logout_removes_it() {
    let dir = scratch("ok");
    let name = World::new(7).snapshot().workspaces[0].name.line().into_string();
    let (out, methods) = against_fake(&dir, &[], &["login"], &paste());
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    assert!(said.contains(&format!("Signed in to {name}")), "{said}");
    assert_eq!(methods, ["client.userBoot"], "one call checks it");
    let file = std::fs::read_to_string(workspaces(&dir)).unwrap();
    assert!(file.contains("host = \"example.slack.com\""), "{file}");
    nothing_secret(&dir, &said);

    let out = slakio(&dir, None, &["logout"], "");
    assert!(out.status.success() && text(&out).contains(&format!("Signed out of {name}")), "{}", text(&out));
    assert!(!workspaces(&dir).exists());
    assert!(text(&slakio(&dir, None, &["logout"], "")).contains("Not signed in"));
}

#[test]
fn a_sign_in_slack_refuses_is_said_and_nothing_is_kept() {
    let dir = scratch("refused");
    let (out, methods) =
        against_fake(&dir, &[("client.userBoot", Fault::Api("invalid_auth".into()))], &["login"], &paste());
    assert!(!out.status.success());
    assert!(text(&out).contains("Slack did not accept this sign-in"), "{}", text(&out));
    assert_eq!(methods, ["client.userBoot"], "never retried on its own");
    assert!(!workspaces(&dir).exists());
    nothing_secret(&dir, &text(&out));
}

#[test]
fn a_paste_without_its_cookie_asks_nothing_of_slack() {
    let dir = scratch("nocookie");
    let (out, methods) = against_fake(&dir, &[], &["login"], &paste().replace(COOKIE, ""));
    assert!(!out.status.success());
    assert!(text(&out).contains("No d cookie in the paste"), "{}", text(&out));
    assert!(methods.is_empty());
    nothing_secret(&dir, &text(&out));
}

#[test]
fn offline_nothing_is_sent_and_it_says_so() {
    let dir = scratch("offline");
    let (out, methods) = against_fake(&dir, &[], &["--offline", "login"], &paste());
    assert!(!out.status.success());
    assert!(text(&out).contains("The network is off"), "{}", text(&out));
    assert!(methods.is_empty(), "the kill switch holds for signing in too");
    assert!(!workspaces(&dir).exists());
}
