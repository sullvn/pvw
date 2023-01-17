use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::io;
use std::sync::mpsc;
use std::thread;

use super::user_input_thread::UserInputEvent;
use super::user_interface_thread::UserInterfaceEvent;

pub enum CommandExitEvent {
    CommandStarted(Pid),
    Stop,
}

pub fn command_exit_thread(
    user_input_events: mpsc::Sender<UserInputEvent>,
    user_interface_events: mpsc::Sender<UserInterfaceEvent>,
    command_exit_events: mpsc::Receiver<CommandExitEvent>,
) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(|| {
        for cee in command_exit_events {
            match cee {
                CommandExitEvent::Stop => break,
                CommandExitEvent::CommandStarted(pid) => {
                    let wait_status = waitpid(pid, Some(WaitPidFlag::WEXITED))?;
                    if let WaitStatus::Exited(pid_exited, exit_code) = wait_status {
                        if pid != pid_exited {
                            return Err(io::Error::new(io::ErrorKind::Other, "Wrong pid exited"));
                        }

                        user_input_events.send(UserInputEvent::CommandExited(pid, exit_code))?;
                        user_interface_events
                            .send(UserInterfaceEvent::CommandExited(pid, exit_code))?;
                    } else {
                        return Err(io::Error::new(
                            io::ErrorKind::Other,
                            "Wrong child process event",
                        ));
                    }
                }
            }
        }

        Ok(())
    })
}
