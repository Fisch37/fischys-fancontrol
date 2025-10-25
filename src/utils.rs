use std::{error::Error, fmt::Display};

#[derive(Debug)]
pub struct SimpleError {
    #[allow(unused)]
    message: String
}
impl SimpleError {
    pub fn new(message: String) -> SimpleError {
        return SimpleError { message: message };
    }
    pub fn from_slice(message: &str) -> SimpleError {
        return SimpleError { message: message.to_string() }
    }
}
impl Display for SimpleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
impl Error for SimpleError { }
impl From<String> for SimpleError {
    fn from(value: String) -> Self {
        SimpleError::new(value)
    }
}

#[derive(Debug)]
pub struct ExitStatusError {
    pub code: Option<i32>,
    pub stderr: Vec<u8>
}
impl Display for ExitStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Exit with code {:?}: {}", self.code, String::from_utf8_lossy(self.stderr.as_slice()))
    }
}
impl Error for ExitStatusError { }

#[derive(Debug)]
pub struct MalformedDataError {
    value: String
}
impl Display for MalformedDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}
impl Error for MalformedDataError { }
impl MalformedDataError {
    pub fn new(message: &str) -> MalformedDataError {
        MalformedDataError::new_with_string(message.to_string())
    }
    pub fn new_with_string(message: String) -> MalformedDataError {
        MalformedDataError {
            value: message
        }
    }
}


#[derive(Debug)]
pub struct ErrorGroup {
    errors: Vec<Box<dyn Error>>
}
impl Display for ErrorGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut res = write!(f, "ErrorGroup[");
        for e in &self.errors {
            res = res.and(write!(f, "{},", e));
        }
        res = res.and(write!(f, "]"));
        return res;
    }
}
impl Error for ErrorGroup { }
impl ErrorGroup {
    pub fn new() -> Self {
        return ErrorGroup { errors: vec![] }
    }

    pub fn push<E: Error + 'static>(&mut self, e: E) {
        self.errors.push(Box::new(e));
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }
}