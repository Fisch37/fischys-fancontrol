use std::{ffi::{c_int, c_uint}, fmt::Display};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Error {
    pub code: i32
}
impl Error {
    /// Converts an i32 into a result with this error type.
    /// If code > 0, Ok(code) will be returned,
    /// else Err with the correct Error structure
    pub(crate) fn convert_cint(code: c_int) -> Result<c_uint> {
        if code < 0 {
            Err(Error { code })
        } else {
            Ok(code as c_uint)
        }
    }
}
impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Unknown error during libsensors call: {}", self.code)
    }
}
impl std::error::Error for Error { }