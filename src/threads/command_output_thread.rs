use nix::pty::PtyMaster;
use std::io::{self, Read};
use std::sync::mpsc;
use std::thread;

use super::user_interface_thread::UserInterfaceEvent;

fn command_output_thread(
    user_interface_events: mpsc::Sender<UserInterfaceEvent>,
    pty_master: PtyMaster,
) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(|| loop {
        let mut output = String::with_capacity(1000);
        pty_master.read_to_string(&mut output)?;
        user_interface_events.send(UserInterfaceEvent::CommandOutput(output))?;
    })
}
