use std::fs::File;
use std::io::Read;
use std::sync::mpsc;
use std::thread;

use super::command_exit_thread::CommandExitEvent;
use super::user_input_thread::UserInputEvent;
use super::user_interface_thread::UserInterfaceEvent;
use crate::result::Result;

pub enum CommandOutputEvent {
    CommandStarted,
    CommandExited,
    Stop,
}

pub fn command_output_thread(
    command_exit_events: mpsc::Sender<CommandExitEvent>,
    user_input_events: mpsc::Sender<UserInputEvent>,
    user_interface_events: mpsc::Sender<UserInterfaceEvent>,
    command_output_events: mpsc::Receiver<CommandOutputEvent>,
    mut pty_master: File,
) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || {
        let result = command_output(
            &user_interface_events,
            &command_output_events,
            &mut pty_master,
        );

        command_exit_events.send(CommandExitEvent::Stop)?;
        user_input_events.send(UserInputEvent::Stop)?;
        user_interface_events.send(UserInterfaceEvent::Stop)?;

        return result;
    })
}

pub fn command_output(
    user_interface_events: &mpsc::Sender<UserInterfaceEvent>,
    command_output_events: &mpsc::Receiver<CommandOutputEvent>,
    pty_master: &mut File,
) -> Result<()> {
    let mut buf: [u8; 1000] = [0; 1000];

    loop {
        match command_output_events.recv()? {
            CommandOutputEvent::Stop => return Ok(()),
            CommandOutputEvent::CommandExited => {}
            CommandOutputEvent::CommandStarted => {
                let read_result = read_command_output(
                    &user_interface_events,
                    &command_output_events,
                    pty_master,
                    &mut buf,
                )?;

                if let ReadCommandResult::Stop = read_result {
                    return Ok(());
                }
            }
        }
    }
}

enum ReadCommandResult {
    Continue,
    Stop,
}

fn read_command_output(
    user_interface_events: &mpsc::Sender<UserInterfaceEvent>,
    command_output_events: &mpsc::Receiver<CommandOutputEvent>,
    pty_master: &mut File,
    output_buffer: &mut [u8],
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

        bytes_read = pty_master.read(output_buffer)?;
        if bytes_read < 1 {
            return Ok(ReadCommandResult::Stop);
        }

        // TODO: Investigate the ramifications of
        //       allowing partial output of control
        //       characters
        //
        // TODO: Properly handle cut-off portions
        //       of valid, multi-byte UTF-8 characters.
        //       Keep the portion around and prepend
        //       it onto the next read result.
        //
        let output = String::from_utf8_lossy(&output_buffer).into_owned();
        user_interface_events.send(UserInterfaceEvent::CommandOutput(output))?;
    }

    Ok(ReadCommandResult::Continue)
}
