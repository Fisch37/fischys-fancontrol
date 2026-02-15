use std::{
    error::Error,
    fmt::{Debug, Display},
};

use log::{debug, error};

use crate::controllers::FanController;

#[derive(Debug)]
pub struct SimpleError {
    #[allow(unused)]
    pub(crate) message: String,
}
impl SimpleError {
    pub fn new(message: String) -> SimpleError {
        SimpleError { message }
    }
    pub fn from_slice<S: ToString>(message: S) -> SimpleError {
        SimpleError {
            message: message.to_string(),
        }
    }
}
impl Display for SimpleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
impl Error for SimpleError {}
impl From<String> for SimpleError {
    fn from(value: String) -> Self {
        SimpleError::new(value)
    }
}

#[derive(Debug)]
pub struct ExitStatusError {
    pub code: Option<i32>,
    pub stderr: Vec<u8>,
}
impl Display for ExitStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Exit with code {:?}: {}",
            self.code,
            String::from_utf8_lossy(self.stderr.as_slice())
        )
    }
}
impl Error for ExitStatusError {}

#[derive(Debug)]
pub struct MalformedDataError {
    value: String,
}
impl Display for MalformedDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}
impl Error for MalformedDataError {}
impl MalformedDataError {
    pub fn new(message: &str) -> MalformedDataError {
        MalformedDataError::new_with_string(message.to_string())
    }
    pub fn new_with_string(message: String) -> MalformedDataError {
        MalformedDataError { value: message }
    }
}

#[derive(Debug)]
pub struct ErrorGroup {
    errors: Vec<Box<dyn Error>>,
}
impl Display for ErrorGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut res = writeln!(f, "ErrorGroup[");
        for e in &self.errors {
            res = res.and(write!(f, "  - {e},"));
        }
        res = res.and(write!(f, "]"));
        res
    }
}
impl Error for ErrorGroup {}
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
    pub fn ok_or_store<I, T, E>(
        &mut self,
        it: I,
    ) -> ErrorGroupConsumeIterator<'_, I::IntoIter, T, E>
    where
        I: IntoIterator<Item = Result<T, E>>,
        E: Error + 'static,
    {
        ErrorGroupConsumeIterator {
            target: self,
            it: it.into_iter(),
        }
    }

    /// Consumes an [`Iterator`] of [`Result`]s and pushes all [`Err`]s onto self,
    /// until the iterator is exhausted or an [`Ok`] is encountered.
    pub fn push_until_ok<I, T, E>(&mut self, it: &mut I) -> Option<T>
    where
        I: Iterator<Item = Result<T, E>>,
        E: Error + 'static,
    {
        for result in it.by_ref() {
            match result {
                Ok(t) => return Some(t),
                Err(e) => self.push(e),
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
pub struct ErrorGroupConsumeIterator<'e, I: Iterator<Item = Result<T, E>>, T, E: Error + 'static> {
    target: &'e mut ErrorGroup,
    it: I,
}
impl<'e, I: Iterator<Item = Result<T, E>>, T, E: Error + 'static> Iterator
    for ErrorGroupConsumeIterator<'e, I, T, E>
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        for result in self.it.by_ref() {
            match result {
                Ok(t) => return Some(t),
                Err(e) => self.target.push(e),
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, self.it.size_hint().1)
    }
}

const MAX_AUTO_RETRIES_ON_EXIT: u8 = 5;
pub fn return_to_auto<C, R>(controllers: &mut [R]) -> usize
where
    C: FanController + ?Sized,
    R: AsMut<C>,
{
    let mut pwms_to_automate: Vec<&mut C> = controllers.iter_mut().map(AsMut::as_mut).collect();
    let mut retries: u8 = MAX_AUTO_RETRIES_ON_EXIT;
    while !pwms_to_automate.is_empty() && retries > 0 {
        let mut pwms_buffer = Vec::new(); // Expected state has 0 failures. Avoids allocation
        for p in pwms_to_automate {
            match p.set_auto(true) {
                Ok(_) => debug!("{} returned to auto", p.get_key()),
                Err(e) => {
                    error!(
                        "Failed to automate {}. {} retries left. Error: {}",
                        p.get_key(),
                        retries,
                        e
                    );
                    pwms_buffer.push(p);
                }
            }
        }
        pwms_to_automate = pwms_buffer;
        retries -= 1;
    }
    pwms_to_automate.len()
}

trait SliceDisplayExtender<'a> {
    type DisplayHelper: Display;

    fn display_join<J: Display>(&self, join_val: &'a J) -> Self::DisplayHelper;
}
impl<'a, 'b, T: Display> SliceDisplayExtender<'b> for &'a [T] {
    type DisplayHelper = SliceJoinDisplay<'a, 'b, T>;

    fn display_join<J: Display>(&self, join_str: &'b J) -> Self::DisplayHelper {
        SliceJoinDisplay {
            slice: self,
            join: join_str,
        }
    }
}
pub struct SliceJoinDisplay<'a, 'b, T: Display> {
    slice: &'a [T],
    join: &'b dyn Display,
}
impl<'a, 'b, T: Display> Display for SliceJoinDisplay<'a, 'b, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut iter = self.slice.iter();
        let mut last_element: Option<&T> = iter.next();
        for element in iter {
            write!(
                f,
                "{}",
                last_element
                    .expect("Slice iter is FusedIterator, last_element should always be Some here")
            )?;
            last_element = Some(element);
            write!(f, "{}", self.join)?;
        }
        match last_element {
            None => {}
            Some(val) => write!(f, "{val}")?,
        }
        Ok(())
    }
}
