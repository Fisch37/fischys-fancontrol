use std::{
    error::Error as StdError,
    io::{stdin, stdout, Write},
};

use crate::{
    controllers::{scan_all, FanController},
    GlobalContext,
};

use self::{
    pwm::cmd_pwm,
    tokenizer::tokenize,
    utils::{err_eol, err_unexpected},
};

mod pwm;
mod tokenizer;
mod utils;

struct RunningContext<'a> {
    pwms: Vec<Box<dyn FanController + 'a>>,
}
impl<'a> RunningContext<'a> {
    pub fn make(ctx: &'a GlobalContext) -> Result<Self, Box<dyn StdError>> {
        Ok(Self {
            pwms: scan_all(ctx)?,
        })
    }
}

pub fn start() {
    let stdin = stdin();
    let global_ctx = GlobalContext::init().unwrap();
    let mut running_ctx = RunningContext::make(&global_ctx).unwrap();

    let mut buf = String::new();
    loop {
        print!("> ");
        let _ = stdout().flush();
        buf.clear();
        stdin.read_line(&mut buf).unwrap();

        if let Err(e) = cmd_entry(&mut running_ctx, &mut tokenize(buf.trim_end())) {
            println!("{e}")
        }
    }
}

fn cmd_entry<'a, I: Iterator<Item = &'a str>>(
    ctx: &mut RunningContext,
    token_stream: &mut I,
) -> Result<(), String> {
    match token_stream.next() {
        Some("pwm") => cmd_pwm(ctx, token_stream),
        Some(s) => err_unexpected(s),
        None => err_eol(),
    }
}
