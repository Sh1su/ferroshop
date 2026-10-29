use serde::Serialize;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A single validation problem, addressed by a JSON pointer into the payload.
#[derive(Debug, Clone, Serialize)]
pub struct WriteError {
    pub pointer: String,
    pub code: &'static str,
    pub detail: String,
}

impl WriteError {
    pub fn new(pointer: impl Into<String>, code: &'static str, detail: impl Into<String>) -> Self {
        Self { pointer: pointer.into(), code, detail: detail.into() }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("entity `{0}` does not exist")]
    UnknownEntity(String),
    #[error("{entity} with id {id} not found")]
    NotFound { entity: String, id: String },
    #[error("invalid criteria: {0}")]
    InvalidCriteria(String),
    #[error("write validation failed")]
    Validation(Vec<WriteError>),
    #[error("{0}")]
    Conflict(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl Error {
    pub(crate) fn criteria(msg: impl Into<String>) -> Self {
        Self::InvalidCriteria(msg.into())
    }

    /// Maps constraint violations to user-facing errors, passes everything else through.
    pub(crate) fn from_db(err: sqlx::Error) -> Self {
        if let sqlx::Error::Database(db) = &err {
            match db.code().as_deref() {
                Some("23505") => {
                    return Self::Validation(vec![WriteError::new(
                        "/",
                        "FRAMEWORK__DUPLICATE_ENTRY",
                        db.message().to_string(),
                    )]);
                }
                Some("23503") => return Self::Conflict(db.message().to_string()),
                Some("23502") => {
                    return Self::Validation(vec![WriteError::new(
                        "/",
                        "FRAMEWORK__WRITE_CONSTRAINT_VIOLATION",
                        db.message().to_string(),
                    )]);
                }
                Some("23514") => {
                    return Self::Validation(vec![WriteError::new(
                        "/",
                        "FRAMEWORK__WRITE_CONSTRAINT_VIOLATION",
                        db.message().to_string(),
                    )]);
                }
                _ => {}
            }
        }
        Self::Database(err)
    }
}
