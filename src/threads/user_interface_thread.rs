use std::process;
use std::string::String;

pub enum UserInterfaceEvent {
    KeyPress(char),
    CommandOutput(String),
    CommandExit(process::ExitStatus),
}
