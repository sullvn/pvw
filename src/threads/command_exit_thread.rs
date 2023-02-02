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
}

pub fn command_exit_thread(
    command_output_events: mpsc::Sender<CommandOutputEvent>,
    user_input_events: mpsc::Sender<UserInputEvent>,
    user_interface_events: mpsc::Sender<UserInterfaceEvent>,
    command_exit_events: mpsc::Receiver<CommandExitEvent>,
) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || {
        for cee in command_exit_events {
            match cee {
                CommandExitEvent::CommandStarted(pid) => {
                    let wait_status = waitpid(pid, None)?;
                    if let WaitStatus::Exited(pid_exited, exit_code) = wait_status {
                        if pid != pid_exited {
                            return Err(
                                io::Error::new(io::ErrorKind::Other, "Wrong pid exited").into()
                            );
                        }

                        command_output_events.send(CommandOutputEvent::CommandExited)?;
                        user_input_events.send(UserInputEvent::CommandExited(pid, exit_code))?;
                        user_interface_events
                            .send(UserInterfaceEvent::CommandExited(pid, exit_code))?;
                    } else {
                        return Err(io::Error::new(
                            io::ErrorKind::Other,
                            "Wrong child process event",
                        )
                        .into());
                    }
                }
            }
        }

        Ok(())
    })
}
