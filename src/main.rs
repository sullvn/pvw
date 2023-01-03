// use nix::sys::termios;
use nix::unistd::isatty;
use std::io::stdin;
use std::os::fd::AsRawFd;

fn main() -> std::io::Result<()> {
    let fd = stdin().as_raw_fd();
    let is_tty = isatty(fd)?;
    if is_tty {
        println!("Is tty");
    } else {
        println!("Is not tty");
    }

    Ok(())
}
