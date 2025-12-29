use std::path::{Path, PathBuf};

use lazy_static::lazy_static;

pub mod fan_configuration;
pub mod fan_discovery;
pub mod groupie;
pub mod pwms;
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

fn main() {
    let subcommand = std::env::args().nth(1).unwrap_or_default();
    match subcommand.as_str() {
        "service" => commands::service::start(),
        "list-sensors" => commands::list_sensors::start(),
        "characteristics" => commands::characteristics::start(),
        _ => println!("Unknown subcommand {}", subcommand)
    }
}