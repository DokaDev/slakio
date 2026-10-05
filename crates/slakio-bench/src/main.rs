//! `slakio-bench`: reproducible measurements of slakio's performance, and the budgets CI holds
//! them to. See `docs/perf.md`.
//!
//! ```text
//! slakio-bench budget [--bin PATH] [--scratch DIR] [--budgets FILE] [--out FILE]
//! ```
//!
//! The scenarios run the release binary in `tmux -L slakio-perf` (a tmux server of its own,
//! never the user's; `SLAKIO_BENCH_TMUX` names another), with every directory it could read or
//! write inside the scratch directory and credentials in memory:
//!
//! * startup — the size of the binary, and the time from process start to the first frame;
//! * idle — event loop wakeups and frames per second while nothing happens.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

const SOCKET: &str = "slakio-perf";
const SOCKET_ENV: &str = "SLAKIO_BENCH_TMUX";

struct Opts {
    bin: PathBuf,
    scratch: PathBuf,
    budgets: PathBuf,
    out: Option<PathBuf>,
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut o = Opts {
        bin: exe.with_file_name(format!("slakio{}", std::env::consts::EXE_SUFFIX)),
        scratch: std::env::temp_dir().join(format!("slakio-bench-{}", std::process::id())),
        budgets: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("budgets.toml"),
        out: None,
    };
    let mut it = args.iter();
    match it.next().map(String::as_str) {
        Some("budget") => {}
        _ => return Err("usage: slakio-bench budget [--bin PATH] [--scratch DIR] [--budgets FILE] [--out FILE]".into()),
    }
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--bin" => o.bin = PathBuf::from(val()?),
            "--scratch" => o.scratch = PathBuf::from(val()?),
            "--budgets" => o.budgets = PathBuf::from(val()?),
            "--out" => o.out = Some(PathBuf::from(val()?)),
            s => return Err(format!("unknown option {s}")),
        }
    }
    Ok(o)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match parse(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    match budget(&opts) {
        Ok(failed) if failed.is_empty() => ExitCode::SUCCESS,
        Ok(failed) => {
            eprintln!("over budget: {}", failed.join(", "));
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Run every scenario and check it against the budgets; the names of the failed checks.
fn budget(o: &Opts) -> Result<Vec<String>, String> {
    let b = load(&o.budgets)?;
    let _ = std::fs::remove_dir_all(&o.scratch);
    let mut checks = Checks::default();
    let start = startup(&o.scratch, &o.bin)?;
    checks.check(
        "binary size",
        start["binary_bytes"].as_f64().unwrap_or(f64::MAX) / 1_048_576.0,
        num(&b, "startup", "binary_mib_max")?,
        " MiB",
    );
    let idle = idle(&o.scratch, &o.bin, num(&b, "idle", "secs")? as u64)?;
    checks.check(
        "idle wakeups",
        idle["wakeups_per_s"].as_f64().unwrap_or(f64::MAX),
        num(&b, "idle", "wakeups_per_s_max")?,
        "/s",
    );
    checks.check(
        "idle frames",
        idle["frames_per_s"].as_f64().unwrap_or(f64::MAX),
        num(&b, "idle", "frames_per_s_max")?,
        "/s",
    );
    let lines = [json!({"scenario": "startup", "result": start}), json!({"scenario": "idle", "result": idle})];
    if let Some(out) = &o.out {
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
        std::fs::write(out, text).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    let _ = std::fs::remove_dir_all(&o.scratch);
    Ok(checks.failed)
}

#[derive(Default)]
struct Checks {
    failed: Vec<String>,
}

impl Checks {
    fn check(&mut self, what: &str, measured: f64, max: f64, unit: &str) {
        let ok = measured <= max;
        println!("  {} {what}: {measured:.3}{unit} (budget {max}{unit})", if ok { "PASS" } else { "FAIL" });
        if !ok {
            self.failed.push(what.to_string());
        }
    }
}

fn load(path: &Path) -> Result<toml::Table, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.parse::<toml::Table>().map_err(|e| format!("{}: {e}", path.display()))
}

/// A number of `section.key` in the budgets.
fn num(b: &toml::Table, section: &str, key: &str) -> Result<f64, String> {
    let v = b.get(section).and_then(|s| s.get(key)).ok_or(format!("budgets: {section}.{key} is missing"))?;
    v.as_float().or_else(|| v.as_integer().map(|i| i as f64)).ok_or(format!("budgets: {section}.{key} is not a number"))
}

fn tmux(args: &[&str]) -> Result<String, String> {
    let socket = std::env::var(SOCKET_ENV).ok().filter(|s| !s.is_empty()).unwrap_or_else(|| SOCKET.to_string());
    let out = Command::new("tmux").arg("-L").arg(socket).args(args).output().map_err(|e| format!("tmux: {e}"))?;
    if !out.status.success() {
        return Err(format!("tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The frame counters the binary wrote (`SLAKIO_BENCH_STATS`).
#[derive(Clone, Copy, Debug, Default)]
struct Counters {
    first_frame_us: u64,
    draws: u64,
    wakeups: u64,
}

fn counters(file: &Path) -> Option<Counters> {
    let text = std::fs::read_to_string(file).ok()?;
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    Some(Counters {
        first_frame_us: v["first_frame_us"].as_u64()?,
        draws: v["draws"].as_u64()?,
        wakeups: v["wakeups"].as_u64()?,
    })
}

/// Start the binary in a new detached tmux session of 120x40 with a private home under
/// `scratch/<name>`; the stats file it writes.
fn spawn(scratch: &Path, name: &str, bin: &Path) -> Result<PathBuf, String> {
    let dir = scratch.join(name);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stats = dir.join("stats.json");
    let q = |p: &Path| format!("'{}'", p.display());
    let command = format!(
        "exec env SLAKIO_SECRET_STORE=memory XDG_CONFIG_HOME={} XDG_DATA_HOME={} XDG_STATE_HOME={} XDG_CACHE_HOME={} SLAKIO_BENCH_STATS={} {}",
        q(&dir.join("config")),
        q(&dir.join("data")),
        q(&dir.join("state")),
        q(&dir.join("cache")),
        q(&stats),
        q(bin)
    );
    let session = format!("slakio-{name}");
    let _ = tmux(&["kill-session", "-t", &session]);
    tmux(&["new-session", "-d", "-s", &session, "-x", "120", "-y", "40", &command])?;
    Ok(stats)
}

fn kill(name: &str) {
    let _ = tmux(&["kill-session", "-t", &format!("slakio-{name}")]);
}

fn wait_for(within: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

fn startup(scratch: &Path, bin: &Path) -> Result<Value, String> {
    let size = std::fs::metadata(bin).map_err(|e| format!("{}: {e}", bin.display()))?.len();
    let stats = spawn(scratch, "startup", bin)?;
    let seen = wait_for(Duration::from_secs(10), || counters(&stats).is_some());
    kill("startup");
    let c = counters(&stats).filter(|_| seen).ok_or("no first frame within 10s")?;
    let first_ms = c.first_frame_us as f64 / 1000.0;
    println!("startup: binary {size} bytes ({:.2} MiB), first frame {first_ms:.1} ms", size as f64 / 1_048_576.0);
    Ok(json!({ "binary_bytes": size, "first_frame_ms": first_ms }))
}

fn idle(scratch: &Path, bin: &Path, secs: u64) -> Result<Value, String> {
    let stats = spawn(scratch, "idle", bin)?;
    let result = (|| {
        if !wait_for(Duration::from_secs(10), || counters(&stats).is_some()) {
            return Err("no first frame within 10s".to_string());
        }
        // Let the start settle (the first frames, a resize), then keep still.
        std::thread::sleep(Duration::from_secs(2));
        let c0 = counters(&stats).unwrap_or_default();
        let t0 = Instant::now();
        std::thread::sleep(Duration::from_secs(secs));
        let elapsed = t0.elapsed().as_secs_f64();
        // The counters are written at most once a second while the loop runs and at exit:
        // quitting writes the totals.
        tmux(&["send-keys", "-t", "slakio-idle", ":qa", "Enter"])?;
        if !wait_for(Duration::from_secs(10), || counters(&stats).is_some_and(|c| c.draws > c0.draws)) {
            return Err("the binary did not quit on :qa".to_string());
        }
        let c1 = counters(&stats).unwrap_or_default();
        // The keys typed to quit are not idle time: each of the four woke the loop, and each
        // but the last (Enter quits before a frame) drew one.
        let wakeups = c1.wakeups.saturating_sub(c0.wakeups).saturating_sub(4) as f64 / elapsed;
        let frames = c1.draws.saturating_sub(c0.draws).saturating_sub(3) as f64 / elapsed;
        println!("idle: {secs}s, {wakeups:.2} wakeups/s, {frames:.2} frames/s");
        Ok(json!({ "secs": secs, "wakeups_per_s": wakeups, "frames_per_s": frames }))
    })();
    kill("idle");
    result
}
