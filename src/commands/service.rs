use std::{collections::HashMap, env, fmt::Write as _, fs, ops::Deref as _, panic::catch_unwind, sync::{Arc, atomic::{AtomicBool, Ordering}}, thread::sleep, time::Duration};

use log::{error, info, warn};
use simple_logger::init_with_env;
use systemd_journal_logger::{connected_to_journal, JournalLog};

use crate::{CONFIG_PATH, CURVES_PATH, DEFAULT_POLL_RATE, GlobalContext, POLL_ENV, controllers::{FanController, scan_all}, curves::{PwmCurve, update_pwms}, groupie::{QueryResult, query_sensors}, utils::return_to_auto};

fn reload_config() -> Result<HashMap<String, PwmCurve>, String> {
    if fs::exists(CURVES_PATH.deref()).map_err(|_| "Could not check for curves file".to_string())? {
        let file = fs::File::open(CURVES_PATH.deref()).map_err(|_| "Could not open curves file".to_string())?;
        match serde_json::from_reader::<_, HashMap<String, PwmCurve>>(&file) {
            Ok(mut x) => {
                x.shrink_to_fit();
                Ok(x)
            },
            Err(e) => Err(format!("Could not parse curves file {e}"))
        }
    } else {
        error!("Fan curves file does not exist. Please run setup");
        Err("Fan curves file does not exist. Please run setup".to_string())
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

fn disable_auto_for_controlled<'a>(controllers: &mut [Box<dyn FanController + 'a>], fan_curves: &HashMap<String, PwmCurve>) {
    'outer: loop {
        // only set fans to manual that are controlled by the given fan curves
        for p in controllers.iter_mut()
            .filter(|p| fan_curves.contains_key(p.get_key()))
        {
            match p.set_auto(false).and_then(|_| p.write_value(p.get_max_value())) {
                Ok(_) => { },
                Err(e) => {
                    warn!("Failed to set pwm {} to manual mode. Trying again. Error: {}", p.get_key(), e);
                    continue 'outer;
                }
            }
        }
        break;
    }
}

fn start_inner(context: GlobalContext) -> Result<(), String> {
    let must_reload_config = Arc::new(AtomicBool::new(false));
    match signal_hook::flag::register(signal_hook::consts::SIGHUP, must_reload_config.clone()) {
        Ok(_) => { },
        Err(e) => error!("Failed to register signal handler for SIGHUP. Config cannot reload. Error {e}")
    }
    let must_exit = Arc::new(AtomicBool::new(false));
    match signal_hook::flag::register(signal_hook::consts::SIGTERM, must_exit.clone()) {
        Ok(_) => { },
        Err(e) => error!("Failed to register signal handler for SIGTERM. Exit will not reset to auto. Error {e}")
    }
    match signal_hook::flag::register(signal_hook::consts::SIGINT, must_exit.clone()) {
        Ok(_) => { },
        Err(e) => error!("Failed to register signal handler for SIGINT. Exit may not reset to auto. Error {e}")
    }

    if !fs::exists(CONFIG_PATH.deref()).unwrap_or(false) {
        fs::create_dir(CONFIG_PATH.deref()).map_err(|e| format!("Could not create config directory: {e}"))?;
    }
    let poll_frequency = env::var(POLL_ENV)
        .map_or(DEFAULT_POLL_RATE, |v| v.parse().unwrap_or(DEFAULT_POLL_RATE));

    let mut fan_curves: HashMap<String, PwmCurve> = reload_config()?;
    
    let mut state = QueryResult::new();
    let mut controllers;
    loop {
        match scan_all(&context) {
            Ok(p) => {
                controllers = p;
                break;
            },
            Err(e) => error!("Failed to find PWM devices. Retrying in 1 second. Error {}", e)
        }
        sleep(Duration::from_secs(1));
    }
    disable_auto_for_controlled(&mut controllers, &fan_curves);
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
                Ok(f) => {
                    fan_curves = f;
                    // need to rerun this, because reload may have changed the affected fans
                    return_to_auto(&mut controllers);
                    disable_auto_for_controlled(&mut controllers, &fan_curves);
                },
                Err(_) => error!("Failed to reload config. Keeping old config just in case.")
            }
        }
        match query_sensors(&mut state, &context) {
            Ok(_) => { },
            Err(e) => warn!("Failed to query sensor state: {}", e)
        }
        match update_pwms(&state, &mut controllers, &fan_curves, &mut |_| {}) {
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

    let count_failed = return_to_auto(&mut controllers);
    if count_failed > 0 {
        warn!("Failed to return {count_failed} fans to auto mode")
    }
    Ok(())
}

// waiting before restart prevents consuming MASSIVE cpu when a permanent issue occurs (like broken config)
const RESTART_WAIT: Duration = Duration::from_secs(2);
pub fn start() {
    match start_logger() {
        Ok(_) => info!("Hello logging!"),
        Err(e) => println!("Logging setup failed. Yeesh. {e}")
    }

    loop {
        // Now GlobalContext always re-initialises when the service panics.
        // I guess this is bad?
        // But, I mean, essentially we now reset on panic, right? So this is completely fine, no?
        match catch_unwind(|| start_inner(GlobalContext::init().unwrap())) {
            Ok(Ok(_)) => {
                info!("Exited regularly");
                break
            },
            Ok(Err(e)) => {
                error!("Service routine exited with an error. Waiting for a while, then restarting it. Error: {e}");
            },
            Err(_) => {
                error!("Service panicked! Waiting a while, then attempting a restart.")
            }
        }
        sleep(RESTART_WAIT);
    }
}