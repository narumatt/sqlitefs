use std::error::Error as stdError;
use rusqlite;

#[derive(Debug, Fail)]
pub enum SqError {
    #[fail(display = "A sqlite error occured: {}", description)]
    SqliteError{description: String},
}

impl From<rusqlite::Error> for SqError {
    fn from(err: rusqlite::Error) -> SqError {
        SqError::SqliteError {description: String::from(format!("{} {:?}",err.description(), err))}
    }
}
