use std::{error::Error, fmt::Display};

use log::{debug, error};

use crate::controllers::FanController;

#[derive(Debug)]
pub struct SimpleError {
    #[allow(unused)]
    message: String
}
impl SimpleError {
    pub fn new(message: String) -> SimpleError {
        SimpleError { message }
    }
    pub fn from_slice(message: &str) -> SimpleError {
        SimpleError { message: message.to_string() }
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
        res
    }
}
impl Error for ErrorGroup { }
impl ErrorGroup {
    pub const fn new() -> Self {
        ErrorGroup { errors: vec![] }
    }

    pub fn push<E: Error + 'static>(&mut self, e: E) {
        self.errors.push(Box::new(e));
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }
}
impl Default for ErrorGroup {
    fn default() -> Self {
        Self::new()
    }
}

const MAX_AUTO_RETRIES_ON_EXIT: u8 = 5;
pub fn return_to_auto<'a>(controllers: &mut [Box<dyn FanController + 'a>]) -> usize {
    let mut pwms_to_automate: Vec<&mut Box<_>> = controllers.iter_mut().collect();
    let mut retries: u8 = MAX_AUTO_RETRIES_ON_EXIT;
    while !pwms_to_automate.is_empty() && retries > 0 {
        let mut pwms_buffer = Vec::new(); // Expected state has 0 failures. Avoids allocation
        for p in pwms_to_automate {
            match p.set_auto(true) {
                Ok(_) => debug!("{} returned to auto", p.get_key()),
                Err(e) => {
                    error!("Failed to automate {}. {} retries left. Error: {}", p.get_key(), retries, e);
                    pwms_buffer.push(p); // Not quite happy about this clone, but its effect should be minimal
                }
            }
        }
        pwms_to_automate = pwms_buffer;
        retries -= 1;
    }
    return pwms_to_automate.len();
}