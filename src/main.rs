use nix::sys::termios;
use nix::unistd::isatty;
use std::io::stdin;
use std::io::stdout;
use std::io::BufReader;
use std::io::Write;
use std::os::fd::AsRawFd;
use utf8::BufReadDecoder;

fn main() -> std::io::Result<()> {
    let stdin = stdin();
    let mut stdout = stdout();

    let stdin_fd = stdin.as_raw_fd();
    let stdout_fd = stdout.as_raw_fd();

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

    let mut term_config = termios::tcgetattr(stdin_fd)?;
    termios::cfmakeraw(&mut term_config);
    termios::tcsetattr(stdin_fd, termios::SetArg::TCSANOW, &term_config)?;

    let mut term_config = termios::tcgetattr(stdout_fd)?;
    termios::cfmakeraw(&mut term_config);
    termios::tcsetattr(stdout_fd, termios::SetArg::TCSANOW, &term_config)?;

    let mut reader = BufReadDecoder::new(BufReader::new(stdin));
    let mut char_buf = [0; 4];

    while let Some(maybe_str) = reader.next_lossy() {
        match maybe_str {
            Err(err) => return Err(err),
            Ok(str) => {
                for c in str.chars() {
                    match c {
                        'q' => {
                            return Ok(());
                        }
                        _ => {
                            stdout.write_all(c.encode_utf8(&mut char_buf).as_bytes())?;
                        }
                    }
                }

                stdout.flush()?;
            }
        }
    }

    Ok(())
}
