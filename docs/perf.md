# Performance budgets

`crates/slakio-bench` measures the release binary and holds it to
`crates/slakio-bench/budgets.toml`. CI's `perf` job runs it on Linux.

```sh
cargo build --release -p slakio-tui -p slakio-bench
./target/release/slakio-bench budget --scratch /tmp/slakio-perf --out /tmp/perf.jsonl
```

The scenarios run the binary in `tmux -L slakio-perf`, a tmux server of their own
(`SLAKIO_BENCH_TMUX` names another one), with every directory pointed into the scratch folder
and credentials in memory. The binary writes its counters to the file named by
`SLAKIO_BENCH_STATS` (off when unset).

| Scenario | Measures | Budget |
|---|---|---|
| startup | size of the release binary; time from process start to the first frame | binary size (MiB) |
| idle | event loop wakeups and frames per second while nothing happens | wakeups/s, frames/s |

One count is held by a test instead (`crates/slakio-tui/tests/pane_flows.rs`): a frame lays out
at most twice the message rows it shows, on the 10,000-message channel and the 1,200-reply
thread, after moving, `g g` and `G` (the drawing code counts the rows it lays out). Messages are
sanitised once, when their page arrives, never per frame. The sanitiser itself reads at most a
fixed number of bytes per name or message and does a few steps per byte, however hostile the
text; its tests count the steps rather than time them (`crates/slakio-core/src/sanitize/tests.rs`).

## Why it does not flake

CI holds **counts** only: bytes, wakeups, frames. Those do not depend on how fast or busy the
runner is. Wall-clock times (first frame, keystroke to frame) are measured and printed, and
checked by hand on a developer machine when a change could affect them; they never fail CI.

## Changing a budget

Measure first, on a developer machine and in CI, and write the measured number and the reason
next to the budget. Keep headroom, but a budget is a limit, not a wish: raise it only with a
measurement and a reason in the same change.
