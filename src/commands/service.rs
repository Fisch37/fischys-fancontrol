use std::{
    collections::HashMap,
    env,
    error::Error as StdError,
    fmt::{Display, Write as _},
    fs,
    io::ErrorKind as IOErrorKind,
    ops::Deref as _,
    panic::catch_unwind,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::sleep,
    time::Duration,
};

use log::{error, info, warn};
use signal_hook::{consts::signal, iterator::Signals};
use simple_logger::init_with_env;
use systemd_journal_logger::{JournalLog, connected_to_journal};

use crate::{
    CONFIG_PATH, CURVES_PATH, DEFAULT_POLL_RATE, GlobalContext, POLL_ENV,
    controllers::{FanController, guards::AutoGuard, scan_all},
    curves::{PwmCurve, update_pwms},
    groupie::SensorStorage,
    utils::return_to_auto,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum ServiceErrorKind {
    Recoverable,
    NeedsIntervention,
}
#[derive(Debug)]
struct ServiceError {
    error: Box<dyn StdError>,
    context: Option<&'static str>,
    kind: ServiceErrorKind,
}
impl ServiceError {
    #[inline]
    fn new<E: StdError + 'static>(
        kind: ServiceErrorKind,
        context: Option<&'static str>,
        e: E,
    ) -> Self {
        ServiceError {
            error: Box::new(e),
            context,
            kind,
        }
    }

    #[inline]
    pub fn with_context<E: StdError + 'static>(
        kind: ServiceErrorKind,
        context: &'static str,
        e: E,
    ) -> Self {
        Self::new(kind, Some(context), e)
    }

    // I'm sure I'll need these eventually
    #[allow(dead_code)]
    pub fn recoverable<E: StdError + 'static>(e: E) -> Self {
        Self::new(ServiceErrorKind::Recoverable, None, e)
    }

    #[allow(dead_code)]
    pub fn recoverable_ctx<E: StdError + 'static>(context: &'static str, e: E) -> Self {
        Self::with_context(ServiceErrorKind::Recoverable, context, e)
    }

    #[allow(dead_code)]
    pub fn needs_intervention<E: StdError + 'static>(e: E) -> ServiceError {
        Self::new(ServiceErrorKind::NeedsIntervention, None, e)
    }

    pub fn needs_intervention_ctx<E: StdError + 'static>(context: &'static str, e: E) -> Self {
        Self::with_context(ServiceErrorKind::NeedsIntervention, context, e)
    }

    pub fn kind(&self) -> ServiceErrorKind {
        self.kind
    }
}
impl Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.context {
            None => write!(f, "{}", self.error),
            Some(context) => write!(f, "{context}: {}", self.error),
        }
    }
}
impl StdError for ServiceError {}

fn start_logger() -> Result<(), String> {
    {
        if connected_to_journal() {
            JournalLog::new()
                .map_err(|e| e.to_string())
                .and_then(|log| {
                    log.install()
                        .map_err(|e| e.to_string())
                        .inspect(|_| info!("Started JournalLog"))
                })
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

fn reload_config() -> Result<HashMap<String, PwmCurve>, ServiceError> {
    let file = fs::File::open(CURVES_PATH.deref()).map_err(|e| {
        ServiceError::with_context(
            match e.kind() {
                IOErrorKind::NotFound => {
                    error!("Fan curves file does not exist. Please run setup");
                    ServiceErrorKind::NeedsIntervention
                }
                IOErrorKind::PermissionDenied => ServiceErrorKind::NeedsIntervention,
                _ => ServiceErrorKind::Recoverable, // assume recoverable, because of spurious I/O errors
            },
            "Could not open config file",
            e,
        )
    })?;
    serde_json::from_reader::<_, HashMap<String, PwmCurve>>(&file)
        .map(|mut x| {
            x.shrink_to_fit();
            x
        })
        .map_err(|e| ServiceError::needs_intervention_ctx("Failed to parse config file", e))
}

fn disable_auto_for_controlled<'a>(
    controllers: &mut [Box<dyn FanController + 'a>],
    fan_curves: &HashMap<String, PwmCurve>,
) {
    'outer: loop {
        // only set fans to manual that are controlled by the given fan curves
        for p in controllers
            .iter_mut()
            .filter(|p| fan_curves.contains_key(p.get_key()))
        {
            if let Err(e) = p
                .set_auto(false)
                .and_then(|_| p.write_value(p.get_max_value()))
            {
                warn!(
                    "Failed to set pwm {} to manual mode. Trying again. Error: {}",
                    p.get_key(),
                    e
                );
                continue 'outer;
            }
        }
        break;
    }
}

fn start_inner(context: GlobalContext) -> Result<(), ServiceError> {
    let must_reload_config = Arc::new(AtomicBool::new(false));
    if let Err(e) =
        signal_hook::flag::register(signal_hook::consts::SIGHUP, must_reload_config.clone())
    {
        error!("Failed to register signal handler for SIGHUP. Config cannot reload. Error {e}")
    }
    let must_exit = Arc::new(AtomicBool::new(false));
    if let Err(e) = signal_hook::flag::register(signal_hook::consts::SIGTERM, must_exit.clone()) {
        error!(
            "Failed to register signal handler for SIGTERM. Exit will not reset to auto. Error {e}"
        )
    }
    if let Err(e) = signal_hook::flag::register(signal_hook::consts::SIGINT, must_exit.clone()) {
        error!(
            "Failed to register signal handler for SIGINT. Exit may not reset to auto. Error {e}"
        )
    }

    if !fs::exists(CONFIG_PATH.deref()).unwrap_or(false) {
        fs::create_dir(CONFIG_PATH.deref()).map_err(|e| {
            ServiceError::needs_intervention_ctx("Could not create config directory", e)
        })?;
    }
    let poll_frequency = env::var(POLL_ENV).map_or(DEFAULT_POLL_RATE, |v| {
        v.parse().unwrap_or(DEFAULT_POLL_RATE)
    });

    let mut fan_curves: HashMap<String, PwmCurve> = reload_config()?;

    let mut state = SensorStorage::new(&context);
    {
        let mut controllers = AutoGuard::wrap(
            loop {
                match scan_all(&context) {
                    Ok(p) => {
                        break p;
                    }
                    Err(e) => error!(
                        "Failed to find PWM devices. Retrying in 1 second. Error {}",
                        e
                    ),
                }
                sleep(Duration::from_secs(1));
            }
        );
        disable_auto_for_controlled(&mut controllers, &fan_curves);
        while !must_exit.load(Ordering::Relaxed) {
            // To be honest I am quite unsure of the Ordering contraints I chose here.
            // My rational is as follows: On a failure (no reload was requested), a signal handler may set the flag.
            // This is fine, as then I shall simply perform the reload during the next iteration.
            // On a success, things must be more stringent. I must never miss a signal handler, therefore I should enforce the strongest possible Ordering.
            // The strongest possible ordering is Acquire-Release, so I chose it.
            // (an Errored compare_exhange operation [.is_ok() == false] is ignored here, since we can just retry next iteration)
            // My unsureness about this is slightly embarassing, as this was actually a topic in my last semester (which is only a few months ago)
            if must_reload_config
                .compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                fan_curves = reload_config()?;
                // TODO: Add some system to determine which fans need to be dropped...
                return_to_auto(&mut controllers);
                disable_auto_for_controlled(&mut controllers, &fan_curves);
            }
            if let Err(e) = state.update() {
                warn!("Failed to query sensor state: {}", e)
            }
            if let Err(errors) =
                update_pwms(&state, &mut controllers, &fan_curves, &mut |_| {})
            {
                // I would have preferred cleaner handling
                let mut output = "[".to_owned();
                for e in errors {
                    if write!(&mut output, "{},", e).is_err() {
                        output += &e.to_string() // Good enough (I don't think this is ever called)
                    }
                }
                output.push(']');
                error!("One or more fan adjustments failed: {}", output)
            }
            sleep(Duration::from_secs(poll_frequency));
        }
    }
    Ok(())
}

// waiting before restart prevents consuming MASSIVE cpu when a permanent issue occurs (like broken config)
const RESTART_WAIT: Duration = Duration::from_secs(2);
pub fn start() {
    match start_logger() {
        Ok(_) => info!("Hello logging!"),
        Err(e) => println!("Logging setup failed. Yeesh. {e}"),
    }

    loop {
        // Now GlobalContext always re-initialises when the service panics.
        // I guess this is bad?
        // But, I mean, essentially we now reset on panic, right? So this is completely fine, no?
        match catch_unwind(|| start_inner(GlobalContext::init().unwrap())) {
            Ok(Ok(_)) => {
                info!("Exited regularly");
                break;
            }
            Ok(Err(e)) => {
                match e.kind() {
                    ServiceErrorKind::Recoverable => {
                        error!(
                            "Service routine exited with an error. Waiting for a while, then restarting it. Error: {e}"
                        );
                        sleep(RESTART_WAIT);
                    }
                    ServiceErrorKind::NeedsIntervention => {
                        error!(
                            "Encountered an error that needs manual intervention. Waiting on reload.\nError: {e}"
                        );
                        let mut hup_listener =
                            Signals::new([signal::SIGHUP, signal::SIGINT, signal::SIGTERM]).expect(
                                "Can't register SIGHUP, SIGINT, SIGTERM handler for reload guard!",
                            );
                        // Wait for next SIGHUP to continue
                        match hup_listener.pending().chain(hup_listener.forever()).next() {
                            Some(signal::SIGHUP) => {}
                            Some(signal::SIGINT | signal::SIGTERM) => {
                                info!(
                                    "Received exit signal while waiting on SIGHUP. Exiting normally"
                                );
                                break;
                            }
                            Some(sig) => warn!("Received unexpected signal {sig}. Wtf?"),
                            None => warn!(
                                "Somehow exited signal listener without receiving a signal. Something is off here..."
                            ),
                        }
                    }
                }
            }
            Err(_) => {
                error!("Service panicked! Waiting a while, then attempting a restart.");
                sleep(RESTART_WAIT);
            }
        }
    }
}
