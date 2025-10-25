pub mod fan_configuration;
pub mod fan_discovery;
pub mod groupie;
pub mod pwms;
pub mod curves;
pub mod utils;
mod commands;

pub const APP_ID: &str = "fischys-fancontrol";
pub const CONFIG_PATH: &str = "/etc/fischys-fancontrol";
pub const CHARACTERISTICS_FILE: &str = "fan-characteristics.json";
pub const CURVES_FILE: &str = "fan-curves.json";
pub const POLL_ENV: &str = "POLL_RATE";
pub const DEFAULT_POLL_RATE: u64 = 3;

fn main() {
    let subcommand = std::env::args().nth(1).unwrap_or_default();
    match subcommand.as_str() {
        "service" => commands::service::start(),
        "list-sensors" => commands::list_sensors::start(),
        "characteristics" => commands::characteristics::start(),
        _ => println!("Unknown subcommand {}", subcommand)
    }
}