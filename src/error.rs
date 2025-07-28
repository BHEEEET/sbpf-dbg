use thiserror::Error;

#[derive(Debug, Error)]
pub enum DebuggerError {
    #[error("Failed to read DWARF data: {0}")]
    ReadError(#[from] gimli::Error),
    #[error("Failed to parse DWARF unit: {0}")]
    UnitError(gimli::Error),
    #[error("Failed to read file: {0}")]
    FileError(#[from] std::io::Error),
    #[error("Failed to parse object: {0}")]
    ObjectError(#[from] object::Error),
}

pub type DebuggerResult<T> = Result<T, DebuggerError>;
