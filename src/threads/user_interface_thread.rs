use nix::sys::termios::{self, Termios};
use nix::unistd::Pid;
use std::io::{self, BufWriter, Stdout, Write};
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::sync::mpsc;
use std::thread;

use super::command_exit_thread::CommandExitEvent;
use super::command_output_thread::CommandOutputEvent;
use super::user_input_thread::UserInputEvent;
use crate::result::Result;

pub enum UserInterfaceEvent {
    KeyPress(char),
    CommandOutput(String),
    CommandExited(Pid, Option<i32>),
    Stop,
}

pub fn user_interface_thread(
    command_exit_events: mpsc::Sender<CommandExitEvent>,
    command_output_events: mpsc::Sender<CommandOutputEvent>,
    user_input_events: mpsc::Sender<UserInputEvent>,
    user_interface_events: mpsc::Receiver<UserInterfaceEvent>,
    stdout: Stdout,
    term_config_original: Termios,
) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || {
        let result = user_interface(&user_interface_events, stdout, term_config_original);

        command_exit_events.send(CommandExitEvent::Stop)?;
        command_output_events.send(CommandOutputEvent::Stop)?;
        user_input_events.send(UserInputEvent::Stop)?;

        return result;
    })
}

fn user_interface(
    user_interface_events: &mpsc::Receiver<UserInterfaceEvent>,
    stdout: Stdout,
    term_config_original: Termios,
) -> Result<()> {
    let stdout_fd = stdout.as_fd().try_clone_to_owned()?;
    let mut stdout = BufWriter::new(stdout);

    let mut term_config_raw = term_config_original.clone();
    termios::cfmakeraw(&mut term_config_raw);

    let mut command_text = String::new();

    // - Erase whole display (keep scrollback)
    // - Move cursor to top
    stdout.write_all("\u{1b}[2J\u{1b}[1;1H".as_bytes())?;
    stdout.flush()?;

    for uie in user_interface_events {
        let user_interface_result = handle_user_interface_event(
            &mut stdout,
            &stdout_fd,
            &mut command_text,
            &term_config_original,
            &term_config_raw,
            uie,
        )?;
        if let UserInterfaceResult::Stop = user_interface_result {
            return Ok(());
        }
    }

    Ok(())
}

enum UserInterfaceResult {
    Continue,
    Stop,
}

fn handle_user_interface_event(
    stdout: &mut BufWriter<Stdout>,
    stdout_fd: &OwnedFd,
    command_text: &mut String,
    term_config_original: &Termios,
    term_config_raw: &Termios,
    event: UserInterfaceEvent,
) -> Result<UserInterfaceResult> {
    match event {
        UserInterfaceEvent::Stop => return Ok(UserInterfaceResult::Stop),
        UserInterfaceEvent::CommandExited(..) => {}
        UserInterfaceEvent::CommandOutput(output) => {
            //
            // - Move down to next line
            // - Clear display
            //
            stdout.write_all("\u{1b}[2;1H\u{1b}[0J".as_bytes())?;
            stdout.flush()?;

            termios::tcsetattr(
                stdout_fd.as_raw_fd(),
                termios::SetArg::TCSANOW,
                &term_config_original,
            )?;
            io::copy(&mut output.as_bytes(), stdout)?;
            stdout.flush()?;

            termios::tcsetattr(
                stdout_fd.as_raw_fd(),
                termios::SetArg::TCSANOW,
                &term_config_raw,
            )?;
        }
        UserInterfaceEvent::KeyPress(char) => {
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

            //
            // - Move cursor to top
            // - Erase line
            // - Print command
            //
            // Using ANSI, not ECH or DCH in Linux console codes:
            //
            // https://man7.org/linux/man-pages/man4/console_codes.4.html
            //
            // TODO
            //
            // Avoid unnecessary redraws by only
            // drawing the difference. Use
            // `unicode_segmentation` to calculate
            // which position to jump to.
            //
            stdout.write_all("\u{1b}[1;1H\u{1b}[0K".as_bytes())?;
            stdout.write_all(command_text.as_bytes())?;
            stdout.write_all("█".as_bytes())?;
            stdout.flush()?;
        }
    }

    Ok(UserInterfaceResult::Continue)
}
