use nix::sys::signal::SIGKILL;
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::Pid;
use std::io;
use std::sync::mpsc;
use std::thread;

use super::command_output_thread::CommandOutputEvent;
use super::user_input_thread::UserInputEvent;
use super::user_interface_thread::UserInterfaceEvent;
use crate::result::Result;

pub enum CommandExitEvent {
    CommandStarted(Pid),
    Stop,
}

pub fn command_exit_thread(
    command_output_events: mpsc::Sender<CommandOutputEvent>,
    user_input_events: mpsc::Sender<UserInputEvent>,
    user_interface_events: mpsc::Sender<UserInterfaceEvent>,
    command_exit_events: mpsc::Receiver<CommandExitEvent>,
) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || {
        if let Err(err) = command_exit(
            &command_output_events,
            &user_input_events,
            &user_interface_events,
            &command_exit_events,
        ) {
            command_output_events.send(CommandOutputEvent::Stop)?;
            user_input_events.send(UserInputEvent::Stop)?;
            user_interface_events.send(UserInterfaceEvent::Stop)?;

            return Err(err);
        }

        Ok(())
    })
}

pub fn command_exit(
    command_output_events: &mpsc::Sender<CommandOutputEvent>,
    user_input_events: &mpsc::Sender<UserInputEvent>,
    user_interface_events: &mpsc::Sender<UserInterfaceEvent>,
    command_exit_events: &mpsc::Receiver<CommandExitEvent>,
) -> Result<()> {
    for cee in command_exit_events {
        match cee {
            CommandExitEvent::Stop => return Ok(()),
            CommandExitEvent::CommandStarted(pid) => {
                let wait_status = waitpid(pid, None)?;
                match wait_status {
                    WaitStatus::Exited(pid_exited, _)
                    | WaitStatus::Signaled(pid_exited, SIGKILL, _) => {
                        let exit_code = if let WaitStatus::Exited(_, exit_code) = wait_status {
                            Some(exit_code)
                        } else {
                            None
                        };

                        if pid != pid_exited {
                            return Err(
                                io::Error::new(io::ErrorKind::Other, "Wrong pid exited").into()
                            );
                        }

                        command_output_events.send(CommandOutputEvent::CommandExited)?;
                        user_input_events.send(UserInputEvent::CommandExited(pid, exit_code))?;
                        user_interface_events
                            .send(UserInterfaceEvent::CommandExited(pid, exit_code))?;
                    }
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("Wrong child process event: {:?}", wait_status),
                        )
                        .into());
                    }
                }
            }
        }
    }

    Ok(())
}
