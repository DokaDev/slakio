//! `:net off` and `:net on`: the network switched off for a while and on again, for the rest of
//! the run (the config is not changed). Off, nothing is sent: the binary switches the process's
//! kill switch, which ends the requests in flight and every live updates socket, and the status
//! line says `Offline` until it is on again. On, the sockets open again by themselves (and catch
//! up), and the workspaces that could not boot meanwhile are asked for again. A network that was
//! off from the start (`--offline`, `SLAKIO_NETWORK=off`, `network = "off"`) stays off: that
//! switch is the user's word for the whole run. The demo sends nothing anyway: it has no `:net`.

use super::{App, Effect, Row};
use slakio_core::i18n::{Label, Msg};
use std::time::Instant;

/// The values `:net` takes.
pub const NET_VALUES: &[&str] = &["on", "off"];

impl App {
    /// There is a network to switch: a backend that is not the demo.
    pub fn networked(&self) -> bool {
        self.backend.is_some_and(|c| !c.demo)
    }

    /// Nothing is sent: the network was switched off, here or for the whole run.
    pub fn offline(&self) -> bool {
        self.networked() && (self.model.net.off || self.model.net.locked)
    }

    /// `:net on` (`on`) or `:net off`.
    pub fn set_net(&mut self, on: bool, now: Instant) {
        if !self.networked() {
            return;
        }
        if on && self.model.net.locked {
            return self.warn(Msg::Label(Label::NetLocked), now);
        }
        self.model.net.off = !on;
        self.effects.push(Effect::Network(on));
        self.info(Msg::Label(if on { Label::NetOn } else { Label::NetOff }), now);
        if on {
            // What failed while it was off may load now: the workspaces, the page shown.
            self.boot_failed_again();
            self.work.retry();
        }
    }

    /// The binary switched the network as asked (`ok`), or could not switch it on (the run's
    /// request cap was reached): then it is off for good, and the status line says so.
    pub fn net_switched(&mut self, ok: bool, now: Instant) {
        if !ok {
            self.model.net.locked = true;
            self.warn(Msg::Label(Label::FailCapReached), now);
        }
    }

    /// The palette's row of `:net <value>`.
    pub(super) fn net_row(&self, i: usize) -> Row {
        let on = NET_VALUES[i] == "on";
        let label = |l: Label| self.i18n.label(l).to_string();
        Row {
            name: NET_VALUES[i].to_string(),
            label: label(if on { Label::NetValueOn } else { Label::NetValueOff }),
            keys: if on != self.offline() { label(Label::PaletteCurrent) } else { String::new() },
        }
    }
}
