use nix::fcntl::OFlag;
use nix::pty::{grantpt, posix_openpt, ptsname, unlockpt, PtyMaster};
use nix::sys::termios::{self, Termios};
use nix::unistd::isatty;
use std::fs::File;
use std::io::{self, stdin, stdout, BufReader, BufWriter, Read, Stdout, Write};
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::process::{self, Command};
use std::string::String;
use std::sync::mpsc;
use std::thread;
use utf8::BufReadDecoder;

enum Event {
    KeyPress(char),
    CommandOutput(String),
    CommandExit(process::ExitStatus),
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
) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(|| {
        let mut utf8_input = BufReadDecoder::new(BufReader::new(user_input));
        while let Some(maybe_str) = utf8_input.next_lossy() {
            let str = maybe_str?;
            for c in str.chars() {
                events.send(Event::KeyPress(c))?;
            }
        }

        Ok(())
    })
}

// On new command:
//
// - Stop old command
//   - Child::kill
// - Wait for old command to finish
//   - Child::wait
// - Drain pty master buffer
//   - include command id with output
//     - PROBLEM: still need to drain to
//       know which command an output is for
//   - put in a delimiter in each command's output
//     - hacky, but may be pretty effective
//     - requires buffering + parsing
//     - TIME: 2 cs (in command process)
//   - stop, then go
//     - stop blocking read with signal()
//       - use mutex so stopper can only interrupt
//         the read
//     - select(), then read() as needed
//     - TIME: ~ 3-5cs
//   - tcflush(fd, TCIOFLUSH)
//     - steps
//       1. call tcflush
//       2. read() is either over, finishing or pending forever
//       3a. if over, we treat new reads as new data
//       3b. if finishing, we treat latest read as old data
//       3c. if pending, we treat new reads as new data
//   - or, wait for drain
//     - select/poll in loop until it looks flushed
//     - read()s continue in background
//       - QUESTION: How does a read consumer
//         know when its done?
//     - scenarios:
//       1. read() block
//           select() done
//       2. read() block -> unblock
//           select() done
//       3. read() block -> unblock
//           select() ready -> select() done
//       4. read() block -> unblock -> select() done
//     - TIME: ~ 2-4s
//     - PROBLEM: how tell difference between
//       scenarios 1 and 2?
//   - concurrently read() (block) and read() non-block
//     - can use read() non-block to check for
//       status
//     - PROBLEM: even if defined behavior, how
//       to guarantee correct-order of merged
//       results?
//   - new pty for each process
//     - PROBLEM: slow af?
//   - pty swap chain
//     - PROBLEM: doesn't really solve the
//       problem
// - Start new command
//   - Command::new
// - Start reading output again
//
//

fn command_output_thread(
    pty_master: PtyMaster,
    events: mpsc::Sender<Event>,
) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(|| loop {
        let mut output = String::with_capacity(1000);
        pty_master.read_to_string(&mut output)?;
        events.send(Event::CommandOutput(output))?;
    })
}

fn command_exit_thread(
    process: process::Child,
    events: mpsc::Sender<Event>,
) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(|| {
        let exit_status = process.wait()?;
        events.send(Event::CommandExit(exit_status))?;
        Ok(())
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

fn handle_command_output(ctx: &mut Context, output: String) -> io::Result<()> {
    // - Move down to next line
    // - Clear display
    ctx.stdout.write_all("\u{1b}[1;2\u{1b}[0J".as_bytes())?;
    termios::tcsetattr(
        ctx.stdout_fd,
        termios::SetArg::TCSANOW,
        &ctx.terminal_config_original,
    )?;

    io::copy(&mut output.as_bytes(), &mut ctx.stdout)?;
    Ok(())
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
    let master_fd = posix_openpt(OFlag::O_RDWR | OFlag::O_NOCTTY)?;

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
    let user_input_thread_handle = user_input_thread(stdin, events_sender.clone());
    let command_output_thread_handle = command_output_thread(master_fd, events_sender.clone());
    let command_exit_thread_handle = command_exit_thread(nil, events_sender);

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
            Event::CommandExit(..) => {}
            Event::KeyPress(char) => handle_key_press(&mut ctx, char)?,
            Event::CommandOutput(output) => handle_command_output(&mut ctx, output)?,
        }

        stdout.flush()?;
        termios::tcsetattr(stdout_fd, termios::SetArg::TCSANOW, &term_config)?;
    }

    //
    // Teardown
    //
    user_input_thread_handle.join()?;
    command_output_thread_handle.join()?;
    command_exit_thread_handle.join()?;

    Ok(())
}
