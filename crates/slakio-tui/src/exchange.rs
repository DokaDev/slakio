//! Between the app and its backend: hand the app's requests to the backend and the backend's
//! answers to the app, until neither has anything left. An answer can ask for more (a page that
//! arrives while `gg` waits for the oldest message asks for the next one), so one round is not
//! enough: the requests an answer queued are sent in the same turn, not when the next key
//! happens to wake the loop up. The binary's loop and the tests both use this.

use crate::app::App;
use slakio_core::backend::Backend;

/// One turn of the exchange. `true` when an answer changed the screen.
pub fn exchange(app: &mut App, backend: &mut dyn Backend) -> bool {
    let mut changed = false;
    loop {
        let commands = app.take_commands();
        for (generation, command) in &commands {
            backend.send(*generation, command.clone());
        }
        let mut answered = false;
        while let Some(envelope) = backend.poll() {
            answered = true;
            changed |= app.on_backend(envelope);
        }
        if commands.is_empty() && !answered {
            return changed;
        }
    }
}
