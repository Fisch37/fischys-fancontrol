pub fn expect_on_off<'a, I: Iterator<Item = &'a str>>(
    token_stream: &mut I,
) -> Result<bool, String> {
    match token_stream.next() {
        Some("on") => Ok(true),
        Some("off") => Ok(false),
        Some(s) => err_unexpected(s),
        None => err_eol(),
    }
}

pub fn err_eol<T>() -> Result<T, String> {
    Err(eol_msg())
}
#[must_use]
pub fn eol_msg() -> String {
    "Expected a command, found EOL".to_string()
}

pub fn err_unexpected<T>(s: &str) -> Result<T, String> {
    Err(format!("Unexpected token {s}"))
}
