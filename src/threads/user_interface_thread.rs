use std::io::{self, BufWriter, Stdout, Write};
use std::process;
use std::string::String;
use std::sync::mpsc;
use std::thread;

pub enum UserInterfaceEvent {
    KeyPress(char),
    CommandOutput(String),
    CommandExit(process::ExitStatus),
}

pub fn user_interface_thread(
    user_interface_events: mpsc::Receiver<UserInterfaceEvent>,
    stdout: Stdout,
) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(|| {
        let mut command_text = String::new();
        let mut stdout = BufWriter::new(stdout);

        // - Erase whole display (keep scrollback)
        // - Move cursor to top
        stdout.write_all("\u{1b}[2J\u{1b}[1;1H".as_bytes())?;
        stdout.flush()?;

        for uie in user_interface_events {
            handle_user_interface_event(&mut stdout, &mut command_text, uie)?;
        }

        Ok(())
    })
}

fn handle_user_interface_event(
    stdout: &mut BufWriter<Stdout>,
    command_text: &mut String,
    event: UserInterfaceEvent,
) -> io::Result<()> {
    match event {
        UserInterfaceEvent::CommandExit(..) => {}
        UserInterfaceEvent::CommandOutput(output) => {
            //
            // - Move down to next line
            // - Clear display
            //
            stdout.write_all("\u{1b}[1;2\u{1b}[0J".as_bytes())?;

            io::copy(&mut output.as_bytes(), stdout);
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
        }
    }

    stdout.flush()?;
    Ok(())
}
