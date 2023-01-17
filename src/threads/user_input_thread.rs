use nix::pty::PtyMaster;
use nix::sys::termios;
use nix::unistd::Pid;
use std::io::{self, BufReader, Read};
use std::os::fd::{AsRawFd, OwnedFd};
use std::process::{self, Command};
use std::sync::mpsc;
use std::thread;
use utf8::BufReadDecoder;

use super::command_exit_thread::CommandExitEvent;
use super::user_interface_thread::UserInterfaceEvent;

pub enum UserInputEvent {
    CommandExited(Pid, i32),
}

pub fn user_input_thread<R: Read + Send>(
    command_exit_events: mpsc::Sender<CommandExitEvent>,
    user_interface_events: mpsc::Sender<UserInterfaceEvent>,
    user_input_events: mpsc::Receiver<UserInputEvent>,
    pty_master: PtyMaster,
    pty_slave_fd: OwnedFd,
    user_input: R,
) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(move || {
        let mut utf8_input = BufReadDecoder::new(BufReader::new(user_input));
        let mut command_text = String::new();
        let mut command_process: Option<process::Child> = None;

        while let Some(maybe_str) = utf8_input.next_lossy() {
            let str = maybe_str?;
            for c in str.chars() {
                on_user_input_character(
                    &command_exit_events,
                    &user_interface_events,
                    &user_input_events,
                    &pty_master,
                    &pty_slave_fd,
                    &mut command_text,
                    &mut command_process,
                    c,
                )?;
            }
        }

        Ok(())
    })
}

fn on_user_input_character(
    command_exit_events: &mpsc::Sender<CommandExitEvent>,
    user_interface_events: &mpsc::Sender<UserInterfaceEvent>,
    user_input_events: &mpsc::Receiver<UserInputEvent>,
    pty_master: &PtyMaster,
    pty_slave_fd: &OwnedFd,
    command_text: &mut String,
    command_process: &mut Option<process::Child>,
    char: char,
) -> io::Result<()> {
    user_interface_events.send(UserInterfaceEvent::KeyPress(char))?;

    if let Some(cp) = command_process.take() {
        cp.kill()?;
    }

    match user_input_events.recv()? {
        UserInputEvent::CommandExited(..) => {}
    }

    termios::tcflush(pty_master.as_raw_fd(), termios::FlushArg::TCIOFLUSH)?;

    // TODO: Dedupe
    match char {
        // Escape, Carriage Return, Newline
        '\u{1b}' | '\r' | '\n' => {}
        // Backspace, Delete
        '\u{8}' | '\u{7f}' => {
            command_text.pop();
        }
        _ => {
            command_text.push(char);
        }
    }
    let mut command_tokens = command_text.split_whitespace();
    let maybe_program = command_tokens.next();
    let args = command_tokens;
    let program = match maybe_program {
        Some(program) => program,
        None => return Ok(()),
    };

    let command_process_new = Command::new(program)
        .args(args)
        .stdin(pty_slave_fd.try_clone()?)
        .stdout(pty_slave_fd.try_clone()?)
        .stderr(pty_slave_fd.try_clone()?)
        .spawn()?;

    command_exit_events.send(CommandExitEvent::CommandStarted(Pid::from_raw(
        command_process_new.id() as i32,
    )))?;

    command_process = &mut Some(command_process_new);

    Ok(())
}
