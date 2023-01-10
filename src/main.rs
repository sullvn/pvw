use nix::fcntl::OFlag;
use nix::pty::{grantpt, posix_openpt, ptsname, unlockpt, PtyMaster};
use nix::sys::termios::{self, Termios};
use nix::unistd::isatty;
use std::fs::File;
use std::io::{self, stdin, stdout, BufReader, BufWriter, Read, Stdout, Write};
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::process::Command;
use std::string::String;
use std::sync::mpsc;
use std::thread;
use utf8::BufReadDecoder;

enum Event {
    KeyPress(char),
}

struct Context {
    command_text: String,
    slave_fd: OwnedFd,
    stdout_fd: RawFd,
    stdout: BufWriter<Stdout>,
    command_output: BufReader<PtyMaster>,
    terminal_config_original: Termios,
}

fn user_input_thread<R: Read>(
    user_input: R,
    events: mpsc::Sender<Event>,
) -> thread::JoinHandle<()> {
    thread::spawn(|| {
        let mut utf8_input = BufReadDecoder::new(BufReader::new(user_input));
        while let Some(maybe_str) = utf8_input.next_lossy() {
            let str = maybe_str?;
            for c in str.chars() {
                events.send(Event::KeyPress(c))?;
            }
        }
    })
}

fn handle_key_press(ctx: &mut Context, char: char) -> io::Result<()> {
    match char {
        // Escape
        '\u{1b}' => Ok(()),
        '\r' | '\n' => {
            let mut command_tokens = ctx.command_text.split_whitespace();
            let maybe_program = command_tokens.next();
            let args = command_tokens;

            if let Some(program) = maybe_program {
                termios::tcsetattr(
                    ctx.stdout_fd,
                    termios::SetArg::TCSANOW,
                    &ctx.terminal_config_original,
                )?;

                let mut new_process = Command::new(program)
                    .args(args)
                    .stdin(ctx.slave_fd.try_clone()?)
                    .stdout(ctx.slave_fd.try_clone()?)
                    .stderr(ctx.slave_fd)
                    .spawn()?;

                let exit_status = new_process.wait()?.code().unwrap_or(0);

                ctx.stdout.write_all("\noutput\n".as_bytes())?;
                io::copy(&mut ctx.command_output, &mut ctx.stdout)?;

                ctx.stdout.write_all("\nexit\n".as_bytes())?;
                ctx.stdout.write_all(exit_status.to_string().as_bytes())?;
            }

            Ok(())
        }
        // Backspace, Delete
        '\u{8}' | '\u{7f}' => {
            ctx.command_text.pop();

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
            ctx.stdout.write_all("\u{1b}[1;1H\u{1b}[0K".as_bytes())?;
            ctx.stdout.write_all(ctx.command_text.as_bytes())?;

            let mut command_tokens = ctx.command_text.split_whitespace();
            let maybe_program = command_tokens.next();
            let args = command_tokens;

            if let Some(program) = maybe_program {
                let mut command = Command::new(program);
                command
                    .args(args)
                    .stdin(ctx.slave_fd.try_clone()?)
                    .stdout(ctx.slave_fd.try_clone()?)
                    .stderr(ctx.slave_fd.try_clone()?);
                let mut new_process = match command.spawn() {
                    Ok(new_process) => new_process,
                    Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
                    Err(err) => return Err(err),
                };

                let exit_status = new_process.wait()?.code().unwrap_or(0);

                // - Move down to next line
                // - Clear display
                // - Render output
                ctx.stdout.write_all("\u{1b}[1;2\u{1b}[0J".as_bytes())?;
                termios::tcsetattr(
                    ctx.stdout_fd,
                    termios::SetArg::TCSANOW,
                    &ctx.terminal_config_original,
                )?;

                ctx.stdout.write_all("\noutput\n".as_bytes())?;
                if let Err(err) = io::copy(&mut ctx.command_output, &mut ctx.stdout) {
                    match err.kind() {
                        io::ErrorKind::WouldBlock => {}
                        err_kind => return Err(err_kind.into()),
                    }
                }

                ctx.stdout.write_all("\nexit\n".as_bytes())?;
                ctx.stdout.write_all(exit_status.to_string().as_bytes())?;
            }

            Ok(())
        }
        _ => {
            ctx.command_text.push(char);

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
            ctx.stdout.write_all("\u{1b}[1;1H\u{1b}[0K".as_bytes())?;
            ctx.stdout.write_all(ctx.command_text.as_bytes())?;

            let mut command_tokens = ctx.command_text.split_whitespace();
            let maybe_program = command_tokens.next();
            let args = command_tokens;

            if let Some(program) = maybe_program {
                let mut command = Command::new(program);
                command
                    .args(args)
                    .stdin(ctx.slave_fd.try_clone()?)
                    .stdout(ctx.slave_fd.try_clone()?)
                    .stderr(ctx.slave_fd.try_clone()?);
                let mut new_process = match command.spawn() {
                    Ok(new_process) => new_process,
                    Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
                    Err(err) => return Err(err),
                };

                let exit_status = new_process.wait()?.code().unwrap_or(0);

                // - Move down to next line
                // - Clear display
                // - Render output
                ctx.stdout.write_all("\u{1b}[1;2\u{1b}[0J".as_bytes())?;
                termios::tcsetattr(
                    ctx.stdout_fd,
                    termios::SetArg::TCSANOW,
                    &ctx.terminal_config_original,
                )?;

                ctx.stdout.write_all("\noutput\n".as_bytes())?;
                if let Err(err) = io::copy(&mut ctx.command_output, &mut ctx.stdout) {
                    match err.kind() {
                        io::ErrorKind::WouldBlock => {}
                        err_kind => return Err(err_kind.into()),
                    }
                }

                ctx.stdout.write_all("\nexit\n".as_bytes())?;
                ctx.stdout.write_all(exit_status.to_string().as_bytes())?;
            }

            Ok(())
        }
    }
}

fn main() -> std::io::Result<()> {
    //
    // Input processing
    //
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

    //
    // Terminal configuration
    //
    let mut term_config = termios::tcgetattr(stdout_fd)?;
    let terminal_config_original = term_config.clone();
    termios::cfmakeraw(&mut term_config);
    termios::tcsetattr(stdout_fd, termios::SetArg::TCSANOW, &term_config)?;

    let mut term_config = termios::tcgetattr(stdin_fd)?;
    termios::cfmakeraw(&mut term_config);
    termios::tcsetattr(stdin_fd, termios::SetArg::TCSANOW, &term_config)?;

    //
    // Pseudoterminal configuration
    //
    let master_fd = posix_openpt(OFlag::O_RDWR | OFlag::O_NOCTTY | OFlag::O_NONBLOCK)?;

    grantpt(&master_fd)?;
    unlockpt(&master_fd)?;

    let slave_path = unsafe { ptsname(&master_fd)? };
    let slave_fd: OwnedFd = File::options()
        .read(true)
        .write(true)
        .open(slave_path)?
        .into();

    //
    // Context
    //
    let mut ctx = Context {
        command_text: String::new(),
        command_output: BufReader::new(master_fd),
        slave_fd,
        stdout,
        stdout_fd,
        terminal_config_original,
    };

    //
    // Threads
    //
    let (events_sender, events_receiver) = mpsc::channel::<Event>();
    let user_input_thread_handle = user_input_thread(stdin, events_sender);

    //
    // Setup
    //
    // - Erase whole display (keep scrollback)
    // - Move cursor to top
    stdout.write_all("\u{1b}[2J\u{1b}[1;1H".as_bytes())?;
    stdout.flush()?;

    //
    // Event loop
    //
    for event in events_receiver {
        match event {
            Event::KeyPress(char) => handle_key_press(&mut ctx, char)?,
        }

        stdout.flush()?;
        termios::tcsetattr(stdout_fd, termios::SetArg::TCSANOW, &term_config)?;
    }

    //
    // Teardown
    //
    user_input_thread_handle.join()?;

    Ok(())
}
