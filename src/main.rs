use nix::fcntl::OFlag;
use nix::pty::{grantpt, posix_openpt, ptsname, unlockpt};
use nix::sys::termios;
use nix::unistd::isatty;
use std::fs::File;
use std::io::{self, stdin, stdout, BufReader, BufWriter, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::process::Command;
use std::string::String;
use utf8::BufReadDecoder;

fn main() -> std::io::Result<()> {
    let stdin = stdin();
    let stdout = stdout();

    let stdin_fd = stdin.as_raw_fd();
    let stdout_fd = stdout.as_raw_fd();

    let mut stdout = BufWriter::new(stdout);

    let is_stdin_tty = isatty(stdin_fd)?;
    if !is_stdin_tty {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "stdin needs to be a tty",
        ));
    }
    let is_stdout_tty = isatty(stdout_fd)?;
    if !is_stdout_tty {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "stdout needs to be a tty",
        ));
    }

    let mut term_config = termios::tcgetattr(stdout_fd)?;
    let term_config_original = term_config.clone();
    termios::cfmakeraw(&mut term_config);
    termios::tcsetattr(stdout_fd, termios::SetArg::TCSANOW, &term_config)?;

    let mut term_config = termios::tcgetattr(stdin_fd)?;
    termios::cfmakeraw(&mut term_config);
    termios::tcsetattr(stdin_fd, termios::SetArg::TCSANOW, &term_config)?;

    let master_fd = posix_openpt(OFlag::O_RDWR | OFlag::O_NOCTTY | OFlag::O_NONBLOCK)?;

    grantpt(&master_fd)?;
    unlockpt(&master_fd)?;

    let slave_path = unsafe { ptsname(&master_fd)? };
    let slave_fd: OwnedFd = File::options()
        .read(true)
        .write(true)
        .open(slave_path)?
        .into();

    let mut reader = BufReadDecoder::new(BufReader::new(stdin));
    let mut output = BufReader::new(master_fd);

    // - Erase whole display (keep scrollback)
    // - Move cursor to top
    stdout.write_all("\u{1b}[2J\u{1b}[1;1H".as_bytes())?;
    stdout.flush()?;

    // State
    let mut command = String::new();

    while let Some(maybe_str) = reader.next_lossy() {
        let str = maybe_str?;
        for c in str.chars() {
            match c {
                // Escape
                '\u{1b}' => {
                    return Ok(());
                }
                '\r' | '\n' => {
                    let mut command_tokens = command.split_whitespace();
                    let maybe_program = command_tokens.next();
                    let args = command_tokens;

                    if let Some(program) = maybe_program {
                        termios::tcsetattr(
                            stdout_fd,
                            termios::SetArg::TCSANOW,
                            &term_config_original,
                        )?;

                        let mut new_process = Command::new(program)
                            .args(args)
                            .stdin(slave_fd.try_clone()?)
                            .stdout(slave_fd.try_clone()?)
                            .stderr(slave_fd)
                            .spawn()?;

                        let exit_status = new_process.wait()?.code().unwrap_or(0);

                        stdout.write_all("\noutput\n".as_bytes())?;
                        io::copy(&mut output, &mut stdout)?;

                        stdout.write_all("\nexit\n".as_bytes())?;
                        stdout.write_all(exit_status.to_string().as_bytes())?;
                    }

                    return Ok(());
                }
                // Backspace, Delete
                '\u{8}' | '\u{7f}' => {
                    command.pop();

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
                    stdout.write_all(command.as_bytes())?;

                    let mut command_tokens = command.split_whitespace();
                    let maybe_program = command_tokens.next();
                    let args = command_tokens;

                    if let Some(program) = maybe_program {
                        let mut command = Command::new(program);
                        command
                            .args(args)
                            .stdin(slave_fd.try_clone()?)
                            .stdout(slave_fd.try_clone()?)
                            .stderr(slave_fd.try_clone()?);
                        let mut new_process = match command.spawn() {
                            Ok(new_process) => new_process,
                            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                            Err(err) => return Err(err),
                        };

                        let exit_status = new_process.wait()?.code().unwrap_or(0);

                        // - Move down to next line
                        // - Clear display
                        // - Render output
                        stdout.write_all("\u{1b}[1;2\u{1b}[0J".as_bytes())?;
                        termios::tcsetattr(
                            stdout_fd,
                            termios::SetArg::TCSANOW,
                            &term_config_original,
                        )?;

                        stdout.write_all("\noutput\n".as_bytes())?;
                        if let Err(err) = io::copy(&mut output, &mut stdout) {
                            match err.kind() {
                                io::ErrorKind::WouldBlock => {}
                                err_kind => return Err(err_kind.into()),
                            }
                        }

                        stdout.write_all("\nexit\n".as_bytes())?;
                        stdout.write_all(exit_status.to_string().as_bytes())?;
                    }
                }
                _ => {
                    command.push(c);

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
                    stdout.write_all(command.as_bytes())?;

                    let mut command_tokens = command.split_whitespace();
                    let maybe_program = command_tokens.next();
                    let args = command_tokens;

                    if let Some(program) = maybe_program {
                        let mut command = Command::new(program);
                        command
                            .args(args)
                            .stdin(slave_fd.try_clone()?)
                            .stdout(slave_fd.try_clone()?)
                            .stderr(slave_fd.try_clone()?);
                        let mut new_process = match command.spawn() {
                            Ok(new_process) => new_process,
                            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                            Err(err) => return Err(err),
                        };

                        let exit_status = new_process.wait()?.code().unwrap_or(0);

                        // - Move down to next line
                        // - Clear display
                        // - Render output
                        stdout.write_all("\u{1b}[1;2\u{1b}[0J".as_bytes())?;
                        termios::tcsetattr(
                            stdout_fd,
                            termios::SetArg::TCSANOW,
                            &term_config_original,
                        )?;

                        stdout.write_all("\noutput\n".as_bytes())?;
                        if let Err(err) = io::copy(&mut output, &mut stdout) {
                            match err.kind() {
                                io::ErrorKind::WouldBlock => {}
                                err_kind => return Err(err_kind.into()),
                            }
                        }

                        stdout.write_all("\nexit\n".as_bytes())?;
                        stdout.write_all(exit_status.to_string().as_bytes())?;
                    }
                }
            }
        }

        stdout.flush()?;
        termios::tcsetattr(stdout_fd, termios::SetArg::TCSANOW, &term_config)?;
    }

    Ok(())
}
