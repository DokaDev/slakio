//! `:net off` and `:net on`: the network switched off for a while and on again, for the rest of
//! the run (the config is not changed). Off, nothing is sent: the binary switches the process's
//! kill switch, which ends the requests in flight and every live updates socket, and the status
//! line says `Offline` until it is on again. On (once the binary did switch it on), the sockets
//! open again by themselves (and catch up), and the workspaces that could not boot meanwhile and
//! the pages that failed meanwhile, in every pane, are asked for again. A network that was
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

    /// `:net on` (`on`) or `:net off`. Off is said at once; on only once the binary switched
    /// it on ([`App::net_switched`]): the run's request cap may keep it off.
    pub fn set_net(&mut self, on: bool, now: Instant) {
        if !self.networked() {
            return;
        }
        if on && self.model.net.locked {
            let why = if self.model.net.capped { Label::NetCapped } else { Label::NetLocked };
            return self.warn(Msg::Label(why), now);
        }
        self.effects.push(Effect::Network(on));
        if !on {
            self.model.net.off = true;
            self.info(Msg::Label(Label::NetOff), now);
        }
    }

    /// The binary switched the network as asked (`ok`), or could not switch it on (`on`; the
    /// run's request cap was reached): then it is off for good, and the status line says so.
    /// Switched on, what failed while it was off is asked for again: the workspaces that could
    /// not boot and the page of every pane (not only the one shown).
    pub fn net_switched(&mut self, on: bool, ok: bool, now: Instant) {
        if !on {
            return;
        }
        if !ok {
            (self.model.net.locked, self.model.net.capped) = (true, true);
            return self.warn(Msg::Label(Label::NetCapped), now);
        }
        self.model.net.off = false;
        self.info(Msg::Label(Label::NetOn), now);
        self.boot_failed_again();
        self.work.retry_all();
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
