use std::{
    collections::BTreeMap,
    fmt::Display,
    fs::File,
    io::{ErrorKind, stdout},
    ops::Deref,
    thread::sleep,
    time::Duration,
};

use crate::{
    CHARACTERISTICS_PATH, GlobalContext,
    controllers::scan_all,
    fan_configuration::{FanProperties, detect_fan_properties},
    fan_discovery::{Pwm2Fan, discover_pwm_fans},
    utils::return_to_auto,
};

const SPINUP_TIME: Duration = Duration::from_secs(7);

type FanAssociations = Vec<Pwm2Fan>;
enum FanAssociationError {
    Discovery(Box<dyn std::error::Error>),
    Storing(FanAssociations, std::io::Error),
}
enum AssociationReadError {
    Deserialisation(serde_json::Error),
    IO(std::io::Error),
}
impl Display for AssociationReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IO(e) => e.fmt(f),
            Self::Deserialisation(e) => e.fmt(f),
        }
    }
}

#[derive(clap::Args)]
pub struct CharacteristicsArgs {
    /// Restricts which controllers to analyze.
    /// If passed at least once, only the passed keys will be tracked.
    /// Note: The grapher will run if at least one key matched.
    ///  This means that if there are invalid keys in your list, they will be ignored!
    #[arg(short, long)]
    pub pwms: Vec<String>,
}

pub fn start(args: CharacteristicsArgs) {
    let context = GlobalContext::init().unwrap();
    let associations = match try_read_discovery() {
        Ok(x) => x,
        Err(AssociationReadError::IO(e)) if e.kind() == ErrorKind::NotFound => {
            match discover_and_save(&context) {
                Ok(x) => x,
                Err(FanAssociationError::Discovery(e)) => {
                    eprintln!("Fan discovery failed! {e}");
                    return;
                }
                Err(FanAssociationError::Storing(x, e)) => {
                    eprintln!(
                        "Failed to store discovered fans. You may want to do something about this. {e}"
                    );
                    x
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to read fan associations: {e}");
            return;
        }
    };

    let mut controllers = scan_all(&context).unwrap();
    // If pwms arg is used, only track those pwms
    if !args.pwms.is_empty() {
        // TODO: This is O(n*m). Investigate whether a faster option exists
        controllers.retain(|c| args.pwms.iter().any(|s| s == c.get_key()));
        if controllers.is_empty() {
            eprintln!("No controllers match the specified keys!");
            return;
        } else {
            eprint!("Testing ");
            for c in &controllers {
                eprint!("{} ", c.get_key());
            }
            eprintln!();
        }
    }
    for pwm in &mut controllers {
        pwm.set_auto(false).unwrap();
        pwm.write_value(pwm.get_max_value()).unwrap();
    }
    eprintln!("Waiting to spin up the fans");
    sleep(SPINUP_TIME);

    let mut fan_characteristics: BTreeMap<&str, Vec<FanProperties>> = BTreeMap::new();
    let mut is_partial = false;
    for Pwm2Fan {
        pwm: pwm_name,
        fans,
    } in &associations
    {
        let pwm = match controllers.iter_mut().find(|p| p.get_key() == pwm_name) {
            Some(x) => x,
            None => {
                eprintln!("Couldn't find {pwm_name}! Skipping it and all its fans :(");
                continue;
            }
        };
        if !fans.is_empty() {
            match detect_fan_properties(pwm.as_mut(), fans, &context) {
                Ok(x) => {
                    fan_characteristics.insert(pwm_name, x);
                }
                Err(e) => {
                    eprintln!("Failed to find properties of fans on {pwm_name}: {e}");
                    is_partial = true;
                }
            }
        } else {
            eprintln!("Skipping unused PWM {pwm_name}");
            fan_characteristics.insert(pwm_name, Vec::new());
        }
    }
    if is_partial {
        eprintln!("WARNING: Result has one or more fan characteristics missing");
    }
    match serde_json::to_writer_pretty(stdout(), &fan_characteristics) {
        Ok(_) => println!(),
        Err(e) => eprintln!("Failed to serialise fan characteristics: {e}"),
    }

    let failed_auto = return_to_auto(&mut controllers);
    if failed_auto > 0 {
        eprintln!("Failed to return {failed_auto} PWMs to auto mode!");
    }
}

fn try_read_discovery() -> Result<FanAssociations, AssociationReadError> {
    File::open(CHARACTERISTICS_PATH.deref())
        .map_err(AssociationReadError::IO)
        .and_then(|file| {
            serde_json::from_reader(file).map_err(AssociationReadError::Deserialisation)
        })
}

fn discover_and_save(context: &GlobalContext) -> Result<FanAssociations, FanAssociationError> {
    discover_pwm_fans(context)
        .map_err(FanAssociationError::Discovery)
        .and_then(|association| {
            // would prefer to do this with some combo of map_err and map,
            // but the Err branch here takes ownership of association so that's impossible
            match try_save_associations(&association) {
                Ok(_) => Ok(association),
                Err(e) => Err(FanAssociationError::Storing(association, e)),
            }
        })
}

fn try_save_associations(association: &FanAssociations) -> std::io::Result<()> {
    File::create(CHARACTERISTICS_PATH.deref()).and_then(|file| {
        serde_json::to_writer_pretty(file, association)
            // the only errors that can occur during serialisation are IO and input is not JSON-compatible.
            // input is JSON compatible => this is accurate
            .map_err(|e| e.into())
    })
}
