use std::{collections::HashMap, env, fmt::Write as _, fs, panic::catch_unwind, path::Path, sync::{atomic::{AtomicBool, Ordering}, Arc}, thread::sleep, time::Duration};

use log::{error, info, warn};
use simple_logger::init_with_env;
use systemd_journal_logger::{connected_to_journal, JournalLog};

use crate::{curves::{update_pwms, PwmCurve}, groupie::{query_sensors, QueryResult}, pwms, CONFIG_PATH, CURVES_FILE, DEFAULT_POLL_RATE, POLL_ENV};

fn reload_config() -> Result<HashMap<String, PwmCurve>, String> {
    let mut curves_file = Path::new(CONFIG_PATH).to_owned();
    curves_file.push(CURVES_FILE);
    if fs::exists(&curves_file).map_err(|_| "Could not check for curves file".to_string())? {
        let file = fs::File::open(curves_file).map_err(|_| "Could not open curves file".to_string())?;
        match serde_json::from_reader::<_, HashMap<String, PwmCurve>>(&file) {
            Ok(mut x) => {
                x.shrink_to_fit();
                Ok(x)
            },
            Err(e) => Err(format!("Could not parse curves file {e}").into())
        }
    } else {
        error!("Fan curves file does not exist. Please run setup");
        return Err("Fan curves file does not exist. Please run setup".to_string());
    }
}

fn start_logger() -> Result<(), String> {
    {
        if connected_to_journal() {
            JournalLog::new().map_err(|e| e.to_string())
                .and_then(|log|
                    log.install()
                    .map_err(|e| e.to_string())
                    .inspect(|_| info!("Started JournalLog"))
                )
        } else {
            Err("Journal not connected".to_string())
        }
    }
    .or_else(|e| {
        error!("{e}");
        init_with_env()
            .map_err(|e| format!("Failed to setup simple logger: {e}"))
            .inspect(|_| info!("JournalLog not available. Using simple logger"))
    })
}

const MAX_AUTO_RETRIES_ON_EXIT: u8 = 5;
fn start_inner() -> Result<(), String> {
    let must_reload_config = Arc::new(AtomicBool::new(false));
    match signal_hook::flag::register(signal_hook::consts::SIGHUP, must_reload_config.clone()) {
        Ok(_) => { },
        Err(e) => error!("Failed to register signal handler for SIGHUP. Config cannot reload. Error {e}")
    }
    let must_exit = Arc::new(AtomicBool::new(false));
    match signal_hook::flag::register(signal_hook::consts::SIGTERM, must_exit.clone()) {
        Ok(_) => { },
        Err(e) => error!("Failed to register signal handler for SIGTERM. Exit will not reset to PWM modes. Error {e}")
    }

    let config_dir = Path::new(CONFIG_PATH);
    if !fs::exists(config_dir).unwrap_or(false) {
        fs::create_dir(config_dir).map_err(|e| format!("Could not create config directory: {e}"))?;
    }
    let poll_frequency = env::var(POLL_ENV)
        .map_or(DEFAULT_POLL_RATE, |v| v.parse().unwrap_or(DEFAULT_POLL_RATE));

    let mut fan_curves: HashMap<String, PwmCurve> = reload_config()?;
    
    let mut state = QueryResult::new();
    let pwms;
    loop {
        match pwms::Pwm::scan() {
            Ok(p) => {
                pwms = p;
                break;
            },
            Err(e) => error!("Failed to find PWM devices. Retrying in 1 second. Error {}", e)
        }
        sleep(Duration::from_secs(1));
    }
    'outer: loop {
        for p in &pwms {
            match p.set_auto(false).and_then(|_| p.set_value(255)) {
                Ok(_) => { },
                Err(e) => {
                    warn!("Failed to set pwm {} to auto mode. Trying again. Error: {}", p.get_name(), e);
                    continue 'outer;
                }
            }
        }
        break;
    }
    while !must_exit.load(Ordering::Relaxed) {
        // To be honest I am quite unsure of the Ordering contraints I chose here.
        // My rational is as follows: On a failure (no reload was requested), a signal handler may set the flag.
        // This is fine, as then I shall simply perform the reload during the next iteration.
        // On a success, things must be more stringent. I must never miss a signal handler, therefore I should enforce the strongest possible Ordering.
        // The strongest possible ordering is Acquire-Release, so I chose it.
        // (an Errored compare_exhange operation [.is_ok() == false] is ignored here, since we can just retry next iteration)
        // My unsureness about this is slightly embarassing, as this was actually a topic in my last semester (which is only a few months ago)
        if must_reload_config.compare_exchange(true, false, Ordering::AcqRel,Ordering::Relaxed).is_ok() {
            match reload_config() {
                Ok(f) => fan_curves = f,
                Err(_) => error!("Failed to reload config. Keeping old config just in case.")
            }
        }
        match query_sensors(&mut state) {
            Ok(_) => { },
            Err(e) => warn!("Failed to query sensor state: {}", e)
        }
        match update_pwms(&state, &pwms, &fan_curves, &mut |_| {}) {
            Ok(_) => { },
            Err(errors) => {
                // I would have preferred cleaner handling
                let mut output = "[".to_owned();
                for e in errors {
                    match write!(&mut output, "{},", e) {
                        Ok(_) => { },
                        Err(_) => output += &e.to_string(),  // Good enough (I don't think this is ever called)
                    }
                }
                output.push(']');
                error!("One or more fan adjustments failed: {}", output)
            }
        }
        sleep(Duration::from_secs(poll_frequency));
    }
    let mut pwms_to_automate = pwms;
    let mut pwms_buffer = Vec::new(); // Expected state has 0 failures. Avoids allocation
    let mut retries: u8 = MAX_AUTO_RETRIES_ON_EXIT;
    while !pwms_to_automate.is_empty() && retries > 0 {
        for p in &pwms_to_automate {
            match p.set_auto(true) {
                Ok(_) => { },
                Err(e) => {
                    error!("Failed to automate {}. {} retries left. Error: {}", p.get_name(), retries, e);
                    pwms_buffer.push(p.clone()); // Not quite happy about this clone, but its effect should be minimal
                }
            }
        }
        (pwms_to_automate, pwms_buffer) = (pwms_buffer, pwms_to_automate);
        retries -= 1;
    }
    Ok(())
}

pub fn start() {
    match start_logger() {
        Ok(_) => info!("Hello logging!"),
        Err(e) => println!("Logging setup failed. Yeesh. {e}")
    }

    loop {
        match catch_unwind(|| start_inner()) {
            Ok(Ok(_)) => {
                info!("Exited regularly");
                break
            },
            Ok(Err(e)) => error!("Service routine exited with an error. Restarting it. Error: {e}"),
            Err(_) => {
                error!("Service panicked! Attempting to restart it!")
            }
        }
    }
}