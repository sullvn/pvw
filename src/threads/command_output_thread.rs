use std::fs::File;
use std::io::Read;
use std::sync::mpsc;
use std::thread;

use super::user_interface_thread::UserInterfaceEvent;
use crate::result::Result;

pub fn command_output_thread(
    user_interface_events: mpsc::Sender<UserInterfaceEvent>,
    mut pty_master: File,
) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || loop {
        let mut output = String::with_capacity(1000);
        pty_master.read_to_string(&mut output)?;
        user_interface_events.send(UserInterfaceEvent::CommandOutput(output))?;
    })
}
