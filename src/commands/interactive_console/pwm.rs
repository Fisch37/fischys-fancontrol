use std::{ops::Deref, str::FromStr};

use regex::Regex;

use crate::{controllers::FanController, utils::ErrorGroup};

use super::{
    utils::{eol_msg, err_eol, err_unexpected, expect_on_off},
    RunningContext,
};

pub fn cmd_pwm<'a, I: Iterator<Item = &'a str>>(
    ctx: &mut RunningContext,
    token_stream: &mut I,
) -> Result<(), String> {
    match token_stream.next() {
        Some("list") => {
            let pwm_pattern = try_get_pwm_regex(token_stream)?;
            for pwm in ctx.pwms.iter().filter(|pwm| {
                pwm_pattern
                    .as_ref()
                    .is_none_or(|pat| pat.is_match(pwm.get_key()))
            }) {
                print_pwm(pwm)?
            }
            Ok(())
        }
        Some("auto") => {
            let targets = find_pwms_mut(ctx, token_stream)?;
            let target_state = expect_on_off(token_stream)?;
            let mut eg = ErrorGroup::new();
            for pwm in targets {
                if let Err(e) = pwm.set_auto(target_state) {
                    eg.push(e)
                }
            }
            eg.ok().map_err(|e| e.to_string())
        }
        Some("set") => {
            let targets = find_pwms_mut(ctx, token_stream)?;
            let target_value = token_stream
                .next()
                .ok_or_else(eol_msg)?
                .parse::<PwmValue>()?;

            let mut eg = ErrorGroup::new();
            for pwm in targets {
                if let Err(e) = pwm.write_value(target_value.to_concrete(pwm)) {
                    eg.push(e)
                }
            }
            eg.ok().map_err(|e| e.to_string())
        }
        Some(s) => err_unexpected(s),
        None => err_eol(),
    }
}

fn try_get_pwm_regex<'a, I: Iterator<Item = &'a str>>(
    token_stream: &mut I,
) -> Result<Option<Regex>, String> {
    match token_stream.next() {
        Some(s) => Regex::new(s)
            .map(Some)
            .map_err(|e| format!("pwm_name is invalid regex: {e}")),
        None => Ok(None),
    }
}

fn find_pwms_mut<'r_ctx, 'g_ctx, 'ts, 's, I: Iterator<Item = &'s str>>(
    ctx: &'r_ctx mut RunningContext<'g_ctx>,
    token_stream: &'ts mut I,
) -> Result<
    impl Iterator<Item = &'r_ctx mut (dyn FanController + 'g_ctx)> + use<'r_ctx, 'g_ctx, I>,
    String,
> {
    let regex = try_get_pwm_regex(token_stream)?.ok_or_else(eol_msg)?;
    Ok(ctx
        .pwms
        .iter_mut()
        .filter(move |pwm| regex.is_match(pwm.get_key()))
        .map(AsMut::as_mut))
}

enum PwmValue {
    Min,
    Max,
    Value(f64),
}
impl PwmValue {
    #[must_use]
    pub fn to_concrete<F>(&self, pwm: &F) -> f64
    where
        F: FanController + ?Sized,
    {
        match self {
            Self::Min => pwm.get_min_value(),
            Self::Max => pwm.get_max_value(),
            Self::Value(x) => *x,
        }
    }
}
impl FromStr for PwmValue {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "min" => Self::Min,
            "max" => Self::Max,
            s => Self::Value(s.parse().map_err(|e| {
                format!("Failed to parse pwm value as float (did you mistype?): {e}")
            })?),
        })
    }
}

fn print_pwm<B, F>(pwm: &B) -> Result<(), String>
where
    B: Deref<Target = F>,
    F: FanController + ?Sized,
{
    let (min, max) = pwm.get_min_max_value();
    println!(
        "{}: ({} [{min} - {max}])",
        pwm.get_key(),
        pwm.read_value().map_err(|e| e.to_string())?,
    );
    Ok(())
}
