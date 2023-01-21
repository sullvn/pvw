use core::fmt::{self, Display, Formatter};
use std::any::Any;
use std::error;
use std::io;
use std::sync::mpsc;

type ThreadPanicError = Box<dyn Any + Send + 'static>;

#[derive(Debug)]
pub enum Error {
    ChannelRecvError,
    ChannelSendError,
    IOError(io::Error),
    NixError(nix::Error),
    ThreadPanicError(ThreadPanicError),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChannelRecvError => write!(f, "ChannelRecvError"),
            Self::ChannelSendError => write!(f, "ChannelSendError"),
            Self::IOError(err) => err.fmt(f),
            Self::NixError(err) => err.fmt(f),
            Self::ThreadPanicError(..) => write!(f, "ThreadPanicError"),
        }
    }
}

impl error::Error for Error {}

impl From<mpsc::RecvError> for Error {
    fn from(_err: mpsc::RecvError) -> Self {
        Self::ChannelRecvError
    }
}

impl<T> From<mpsc::SendError<T>> for Error {
    fn from(_err: mpsc::SendError<T>) -> Self {
        Self::ChannelSendError
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::IOError(err)
    }
}

impl From<nix::Error> for Error {
    fn from(err: nix::Error) -> Error {
        Self::NixError(err)
    }
}

impl From<ThreadPanicError> for Error {
    fn from(err: ThreadPanicError) -> Self {
        Self::ThreadPanicError(err)
    }
}
