use std::{collections::BTreeMap, fmt::Display, fs::File, io::{ErrorKind, stdout}, ops::Deref};


use crate::{CHARACTERISTICS_PATH, GlobalContext, fan_configuration::{FanProperties, detect_fan_properties}, fan_discovery::{Pwm2Fan, discover_pwm_fans}, controllers::{FanController as _, Pwm}};

type FanAssociations = Vec<Pwm2Fan>;
enum FanAssociationError {
    Discovery(Box<dyn std::error::Error>),
    Storing(FanAssociations, std::io::Error)
}
enum AssociationReadError {
    Deserialisation(serde_json::Error),
    IO(std::io::Error)
}
impl Display for AssociationReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IO(e) => e.fmt(f),
            Self::Deserialisation(e) => e.fmt(f)
        }
    }
}

pub fn start() {
    let context = GlobalContext::init().unwrap();
    let associations = match try_read_discovery() {
        Ok(x) => x,
        Err(AssociationReadError::IO(e)) if e.kind() == ErrorKind::NotFound => {
            match discover_and_save(&context) {
                Ok(x) => x,
                Err(FanAssociationError::Discovery(e)) => {
                    eprintln!("Fan discovery failed! {e}");
                    return;
                },
                Err(FanAssociationError::Storing(x, e)) => {
                    eprintln!("Failed to store discovered fans. You may want to do something about this. {e}");
                    x
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to read fan associations: {e}");
            return;
        }
    };

    let mut pwms = Pwm::scan().unwrap();
    for pwm in &mut pwms {
        pwm.set_auto(false).unwrap();
        pwm.write_value(pwm.get_max_value()).unwrap();
    }

    let mut fan_characteristics: BTreeMap<&str, Vec<FanProperties>> = BTreeMap::new();
    let mut is_partial = false;
    for Pwm2Fan {pwm: pwm_name, fans}  in &associations {
        let pwm = match pwms.iter_mut().find(|p| p.get_key() == pwm_name) {
            Some(x) => x,
            None => {
                eprintln!("Couldn't find {pwm_name}! Skipping it and all its fans :(");
                continue;
            }
        };
        if !fans.is_empty() {
            match detect_fan_properties(pwm, fans, &context) {
                Ok(x) => {
                    fan_characteristics.insert(pwm_name, x);
                },
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
        Err(e) => eprintln!("Failed to serialise fan characteristics: {e}")
    }
}

fn try_read_discovery() -> Result<FanAssociations, AssociationReadError> {
    File::open(CHARACTERISTICS_PATH.deref())
        .map_err(AssociationReadError::IO)
        .and_then(|file| serde_json::from_reader(file)
            .map_err(AssociationReadError::Deserialisation)
        )
}

fn discover_and_save(context: &GlobalContext) -> Result<FanAssociations, FanAssociationError> {
    discover_pwm_fans(context)
        .map_err(FanAssociationError::Discovery)
        .and_then(|association| {
            // would prefer to do this with some combo of map_err and map,
            // but the Err branch here takes ownership of association so that's impossible
            match try_save_associations(&association) {
                Ok(_) => Ok(association),
                Err(e) => Err(FanAssociationError::Storing(association, e))
            }
        })
}

fn try_save_associations(association: &FanAssociations) -> std::io::Result<()> {
    File::create(CHARACTERISTICS_PATH.deref())
        .and_then(|file| {
            serde_json::to_writer_pretty(file, association)
                // the only errors that can occur during serialisation are IO and input is not JSON-compatible.
                // input is JSON compatible => this is accurate
                .map_err(|e| e.into())
        })
}
