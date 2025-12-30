use std::path::{Path, PathBuf};

use lazy_static::lazy_static;
use log::warn;

pub mod fan_configuration;
pub mod fan_discovery;
pub mod groupie;
pub mod controllers;
pub mod curves;
pub mod utils;
mod commands;

pub const APP_ID: &str = "fischys-fancontrol";
// TODO: Makes these paths private, replacing with lazy_static PathBufs
pub const CONFIG_PATH: &str = "/etc/fischys-fancontrol";
pub const CURVES_FILE: &str = "fan-curves.json";
pub const POLL_ENV: &str = "POLL_RATE";
pub const DEFAULT_POLL_RATE: u64 = 3;
const CHARACTERISTICS_FILE: &str = "fan-characteristics.json";

lazy_static! {
    pub static ref CHARACTERISTICS_PATH: PathBuf = path_to_config(CHARACTERISTICS_FILE);
    pub static ref CURVES_PATH: PathBuf = path_to_config(CURVES_FILE);
}

fn path_to_config<P: AsRef<Path> + ?Sized>(subpath: &P) -> PathBuf {
    let mut path: PathBuf = Path::new(CONFIG_PATH).into();
    path.push(subpath);
    path.shrink_to_fit();
    path
}

static mut GLOBAL_CONTEXT_CREATED: bool = false;
/// This struct is used to store data that needs to be passed throughout the program.
/// Used instead of statics, to ensure everything that needs to be dropped will be dropped.
/// 
/// Note that despite its name this struct does not guarantee its uniqueness.
/// It is technically possible for the program to create two instances of GlobalContext,
/// even though there is no reason to do so. If that happens, a warning will be issued.
pub struct GlobalContext {
    #[cfg(feature = "nvml")]
    nvml: nvml_wrapper::Nvml
}
impl GlobalContext {
    pub fn init() -> Result<Self, Box<dyn std::error::Error>> {
        // SAFETY: This branch is merely a helper warning, so it needn't be 100% reliably called.
        //  It is technically possible for two threads to create two GlobalContexts at the exact same time.
        //  If that happens, this warning will be omitted and your program will still be slightly bugged silently.
        if unsafe { GLOBAL_CONTEXT_CREATED } {
            warn!("Two global context structs were created within the program lifetime. This is most likely a bug")
        }
        Ok(GlobalContext {
            #[cfg(feature = "nvml")]
            nvml: {
                nvml_wrapper::Nvml::init()?
            }
        }).inspect(|_| unsafe { GLOBAL_CONTEXT_CREATED = true })
    }

    #[cfg(feature = "nvml")]
    pub fn get_nvml(&self) -> &nvml_wrapper::Nvml {
        &self.nvml
    }
}

fn main() {
    let subcommand = std::env::args().nth(1).unwrap_or_default();
    match subcommand.as_str() {
        "service" => commands::service::start(),
        "list-sensors" => commands::list_sensors::start(),
        "characteristics" => commands::characteristics::start(),
        _ => println!("Unknown subcommand {}", subcommand)
    }
}