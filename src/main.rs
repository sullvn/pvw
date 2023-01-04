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

    // - Erase whole display (keep scrollback)
    // - Move cursor to top
    stdout.write_all("\u{1b}[2J\u{1b}[1;1H".as_bytes())?;
    stdout.flush()?;

    while let Some(maybe_str) = reader.next_lossy() {
        let str = maybe_str?;
        for c in str.chars() {
            match c {
                // Escape
                '\u{1b}' | '\r' | '\n' => {
                    return Ok(());
                }
                // Backspace, Delete
                '\u{8}' | '\u{7f}' => {
                    //
                    // 1. Move left one column (Can use control code or backspace)
                    // 2. Erase to end of line
                    //
                    // Using ANSI, not ECH or DCH in Linux console codes:
                    //
                    // https://man7.org/linux/man-pages/man4/console_codes.4.html
                    //
                    stdout.write_all("\u{1b}[1D\u{1b}[0K".as_bytes())?;
                }
                _ => {
                    stdout.write_all(c.encode_utf8(&mut char_buf).as_bytes())?;
                }
            }
        }

        stdout.flush()?;
    }

    Ok(())
}
