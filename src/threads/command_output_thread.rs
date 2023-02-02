use std::fs::File;
use std::io::Read;
use std::sync::mpsc;
use std::thread;

use super::user_interface_thread::UserInterfaceEvent;
use crate::result::Result;

pub enum CommandOutputEvent {
    CommandStarted,
    CommandExited,
    Stop,
}

pub fn command_output_thread(
    user_interface_events: mpsc::Sender<UserInterfaceEvent>,
    command_output_events: mpsc::Receiver<CommandOutputEvent>,
    mut pty_master: File,
) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || loop {
        match command_output_events.recv()? {
            CommandOutputEvent::Stop => return Ok(()),
            CommandOutputEvent::CommandExited => {}
            CommandOutputEvent::CommandStarted => {
                let read_result = read_command_output(
                    &user_interface_events,
                    &command_output_events,
                    &mut pty_master,
                )?;

                if let ReadCommandResult::Stop = read_result {
                    return Ok(());
                }
            }
        }
    })
}

enum ReadCommandResult {
    Continue,
    Stop,
}

fn read_command_output(
    user_interface_events: &mpsc::Sender<UserInterfaceEvent>,
    command_output_events: &mpsc::Receiver<CommandOutputEvent>,
    pty_master: &mut File,
) -> Result<ReadCommandResult> {
    let mut bytes_read = 1;
    while 0 < bytes_read {
        match command_output_events.try_recv() {
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => panic!("Unexpected disconnected mpsc channel"),
            Ok(CommandOutputEvent::CommandStarted) => {
                panic!("Unexpected premature command start")
            }
            Ok(CommandOutputEvent::CommandExited) => return Ok(ReadCommandResult::Continue),
            Ok(CommandOutputEvent::Stop) => return Ok(ReadCommandResult::Stop),
        }

        let mut output = String::with_capacity(1000);

        bytes_read = pty_master.read_to_string(&mut output)?;
        if bytes_read < 1 {
            return Ok(ReadCommandResult::Stop);
        }

        user_interface_events.send(UserInterfaceEvent::CommandOutput(output))?;
    }

    Ok(ReadCommandResult::Continue)
}
