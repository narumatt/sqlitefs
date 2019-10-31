use std::fmt;
use std::fmt::Display;
use std::error::Error as stdError;

use failure::{Backtrace, Context, Fail};

pub type Result<T> = ::std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Error {
    inner: Context<ErrorKind>,
}

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "A sqlite error occured: {}", description)]
    SqliteError{description: String},
    #[fail(display = "Target is directory: {}", description)]
    FsIsDir{description: String},
    #[fail(display = "Target is not directory: {}", description)]
    FsIsNotDir{description: String},
    #[fail(display = "Target is not found: {}", description)]
    FsNoEnt{description: String},
    #[fail(display = "Target is not empty: {}", description)]
    FsNotEmpty{description: String},
    #[fail(display = "Undefined error: {}", description)]
    Undefined{description: String},
}

impl Fail for Error {
    fn cause(&self) -> Option<&dyn Fail> {
        self.inner.cause()
    }

    fn backtrace(&self) -> Option<&Backtrace> {
        self.inner.backtrace()
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.inner, f)
    }
}

impl Error {
    pub fn kind(&self) -> &ErrorKind {
        self.inner.get_context()
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error {
        Error {
            inner: Context::new(kind),
        }
    }
}

impl From<Context<ErrorKind>> for Error {
    fn from(inner: Context<ErrorKind>) -> Error {
        Error { inner }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Error {
        Error {
            inner: Context::new(
                ErrorKind::SqliteError {
                    description: String::from(format!("{} {:?}", err.description(), err))
                }
            )
        }
    }
}
