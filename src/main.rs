use nix::fcntl::OFlag;
use nix::pty::{grantpt, posix_openpt, ptsname, unlockpt};
use nix::sys::termios;
use nix::unistd::isatty;
use std::fs::File;
use std::io::{stdin, stdout};
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::sync::mpsc;

mod error;
mod result;
mod threads;

use crate::result::Result;
use crate::threads::{
    command_exit_thread, command_output_thread, user_input_thread, user_interface_thread,
    CommandExitEvent, CommandOutputEvent, UserInputEvent, UserInterfaceEvent,
};

fn main() -> Result<()> {
    //
    // Input processing
    //
    let stdin = stdin();
    let stdout = stdout();

    let stdin_fd = stdin.as_raw_fd();
    let stdout_fd = stdout.as_raw_fd();

    let is_stdin_tty = isatty(stdin_fd)?;
    if !is_stdin_tty {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "stdin needs to be a tty",
        )
        .into());
    }
    let is_stdout_tty = isatty(stdout_fd)?;
    if !is_stdout_tty {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "stdout needs to be a tty",
        )
        .into());
    }

    //
    // Terminal configuration
    //
    let mut term_config = termios::tcgetattr(stdout_fd)?;
    termios::cfmakeraw(&mut term_config);
    termios::tcsetattr(stdout_fd, termios::SetArg::TCSANOW, &term_config)?;

    let mut term_config = termios::tcgetattr(stdin_fd)?;
    termios::cfmakeraw(&mut term_config);
    termios::tcsetattr(stdin_fd, termios::SetArg::TCSANOW, &term_config)?;

    //
    // Pseudoterminal configuration
    //
    let pty_master = posix_openpt(OFlag::O_RDWR | OFlag::O_NOCTTY)?;

    grantpt(&pty_master)?;
    unlockpt(&pty_master)?;

    let pty_slave_path = unsafe { ptsname(&pty_master)? };
    let pty_slave_fd: OwnedFd = File::options()
        .read(true)
        .write(true)
        .open(pty_slave_path)?
        .into();

    let pty_master_1 = unsafe { File::from_raw_fd(pty_master.into_raw_fd()) };
    let pty_master_2 = pty_master_1.try_clone()?;

    //
    // Threads
    //
    let (command_exit_events_sender, command_exit_events_receiver) =
        mpsc::channel::<CommandExitEvent>();
    let (command_output_events_sender, command_output_events_receiver) =
        mpsc::channel::<CommandOutputEvent>();
    let (user_input_events_sender, user_input_events_receiver) = mpsc::channel::<UserInputEvent>();
    let (user_interface_events_sender, user_interface_events_receiver) =
        mpsc::channel::<UserInterfaceEvent>();

    let command_exit_thread_handle = command_exit_thread(
        command_output_events_sender.clone(),
        user_input_events_sender,
        user_interface_events_sender.clone(),
        command_exit_events_receiver,
    );
    let command_output_thread_handle = command_output_thread(
        user_interface_events_sender.clone(),
        command_output_events_receiver,
        pty_master_1,
    );
    let user_input_thread_handle = user_input_thread(
        command_exit_events_sender,
        command_output_events_sender,
        user_interface_events_sender,
        user_input_events_receiver,
        pty_master_2,
        pty_slave_fd,
        stdin,
    );
    let user_interface_thread_handle =
        user_interface_thread(user_interface_events_receiver, stdout);

    let command_exit_thread_result = command_exit_thread_handle.join()?;
    let command_output_thread_result = command_output_thread_handle.join()?;
    let user_input_thread_result = user_input_thread_handle.join()?;
    let user_interface_thread_result = user_interface_thread_handle.join()?;

    command_exit_thread_result?;
    command_output_thread_result?;
    user_input_thread_result?;
    user_interface_thread_result?;

    Ok(())
}
