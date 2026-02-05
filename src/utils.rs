use std::{error::Error, fmt::Display, ops::{Deref, DerefMut}};

use log::{debug, error, warn};

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
    pub fn from_slice<S: ToString>(message: S) -> SimpleError {
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
        let mut res = write!(f, "ErrorGroup[\n");
        for e in &self.errors {
            res = res.and(write!(f, "  - {e},"));
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
        self.push_box(Box::new(e));
    }
    pub fn push_box(&mut self, e: Box<dyn Error>) {
        self.errors.push(e);
    }

    /// A helper function wrapping an iterator of [`Result`]s.
    /// Skips all [`Err`] variants in the iterator, adding the contained [`Error`] to this [`ErrorGroup`]
    /// and yields only the contents of [`Ok`] variants.
    pub fn ok_or_store<I, T, E>(&mut self, it: I) -> ErrorGroupConsumeIterator<'_, I::IntoIter, T, E>
        where I: IntoIterator<Item = Result<T, E>>, E: Error + 'static
    {
        ErrorGroupConsumeIterator { target: self, it: it.into_iter() }
    }

    /// Consumes an [`Iterator`] of [`Result`]s and pushes all [`Err`]s onto self,
    /// until the iterator is exhausted or an [`Ok`] is encountered.
    pub fn push_until_ok<I, T, E>(&mut self, it: &mut I) -> Option<T>
        where I: Iterator<Item = Result<T, E>>, E: Error + 'static
    {
        while let Some(result) = it.next() {
            match result {
                Ok(t) => return Some(t),
                Err(e) => self.push(e)
            }
        }
        None
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn ok(self) -> Result<(), Self> {
        if self.is_empty() { Ok(()) } else { Err(self) }
    }
}
impl Default for ErrorGroup {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper macro for using an [`ErrorGroup`] in loops.
/// If the result of `expr` is an [`Err`],
/// pushes the error onto the `error_group` and skips this loop iteration.
/// If thr result is an [`Ok`], returns the contained value.
#[macro_export]
macro_rules! eg_push_and_continue {
    ($error_group: ident, $expr: expr) => {
        match $expr {
            Ok(t) => t,
            Err(e) => {
                $error_group.push(e);
                continue;
            }
        }
    };
}
struct ErrorGroupConsumeIterator<'e, I: Iterator<Item = Result<T, E>>, T, E: Error + 'static> {
    target: &'e mut ErrorGroup,
    it: I
}
impl<'e, I: Iterator<Item = Result<T, E>>, T, E: Error + 'static> Iterator for ErrorGroupConsumeIterator<'e, I, T, E> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(result) = self.it.next() {
            match result {
                Ok(t) => return Some(t),
                Err(e) => self.target.push(e)
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, self.it.size_hint().1)
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
                    pwms_buffer.push(p);
                }
            }
        }
        pwms_to_automate = pwms_buffer;
        retries -= 1;
    }
    pwms_to_automate.len()
}

/// This struct is a guard around multiple controllers, that tries to return those controllers to automatic,
/// when it leaves scope. Note that since this uses the [`Drop`] trait, 
/// there is no guarantee that all controllers successfully enter automatic mode.
/// 
/// This struct also implements [`Deref`] and [`DerefMut`] so that access to the contained values is still possible.
pub struct ReturnToAutoWrapper<'a, 'b>
{
    controllers: &'b mut [Box<dyn FanController + 'a>]
}
impl<'a, 'b> ReturnToAutoWrapper<'a, 'b>
{
    pub fn new(controllers: &'b mut [Box<dyn FanController + 'a>]) -> Self {
        Self { controllers }
    }
}
impl<'a, 'b> From<&'b mut [Box<dyn FanController + 'a>]> for ReturnToAutoWrapper<'a, 'b> {
    fn from(value: &'b mut [Box<dyn FanController + 'a>]) -> Self {
        Self::new(value)
    }
}
impl<'a, 'b> Deref for ReturnToAutoWrapper<'a, 'b> {
    type Target = [Box<dyn FanController + 'a>];

    fn deref(&self) -> &Self::Target {
        self.controllers
    }
}
impl<'a, 'b> DerefMut for ReturnToAutoWrapper<'a, 'b> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.controllers
    }
}
impl<'a, 'b> Drop for ReturnToAutoWrapper<'a, 'b> {
    fn drop(&mut self) {
        let failed_count = return_to_auto(self.controllers);
        if failed_count > 0 {
            warn!("Failed to return {failed_count} fan controllers to auto mode!")
        }
    }
}