use std::{thread::sleep, time::Duration};

use regex::Regex;
use tap::prelude::*;

use crate::{
    GlobalContext,
    groupie::{Adapter, Sensor, SensorKey, SensorKind, SensorStorage},
};

#[derive(clap::Args)]
pub struct ListSensorsArgs {
    #[arg(short = 'K', long)]
    kind: Option<SensorKind>,
    #[arg(short = 'a', long)]
    adapter_key: Option<Regex>,
    #[arg(short = 's', long)]
    sensor_key: Option<Regex>,

    #[arg(short = 't', long = "repeat")]
    repeat_interval: Option<u64>,
}
impl ListSensorsArgs {
    pub fn should_list(&self, sensor: &Sensor) -> bool {
        self.kind.is_none_or(|kind| sensor.kind == kind)
            && self
                .adapter_key
                .as_ref()
                .is_none_or(|regex| regex.is_match(&sensor.name))
            && self
                .sensor_key
                .as_ref()
                .is_none_or(|regex| regex.is_match(&sensor.adapter.key))
    }
}

pub fn start(args: ListSensorsArgs) {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Warn)
        .env()
        .init()
        .unwrap();
    let context = GlobalContext::init().unwrap();
    let mut state = SensorStorage::new(&context);

    loop {
        state.update().unwrap();
        let state = state
            .iter()
            .collect::<Vec<_>>()
            .tap_mut(|state| state.sort_by(|(a, _), (b, _)| a.cmp_by_sensor_key(b)));

        let mut last_adapter: Option<&Adapter> = None;
        for (sensor, state) in state.iter().filter(|(sensor, _)| args.should_list(sensor)) {
            if last_adapter.is_none_or(|last_adapter| *last_adapter != *sensor.adapter) {
                println!("\n{}:", sensor.adapter.name);

                last_adapter = Some(&sensor.adapter);
            }

            println!("{}", sensor.name);
            match state {
                None => println!("  N/A"),
                Some(state) => {
                    println!("  Val: {}", format_input(sensor.kind, state.input));
                    println!("  Min: {}", format_input(sensor.kind, state.min));
                    println!("  Max: {}", format_input(sensor.kind, state.max));
                }
            }
        }

        match args.repeat_interval.map(Duration::from_secs) {
            None => break,
            Some(dur) => sleep(dur),
        }
    }
}

fn format_input(kind: SensorKind, input: f64) -> String {
    match kind {
        SensorKind::Temperature => format!("{input:>2.1}°C"),
        SensorKind::Fan => format!("{input:>4.0} RPM"),
        SensorKind::Beep => (if input > 0.0 { "true" } else { "false" }).to_string(),
        SensorKind::Power => format!("{input:.2}W"),
        SensorKind::Voltmeter => format!("{input:.3}V"),
        SensorKind::Current => format!("{input:.3}A"),
        SensorKind::Energy => format!("{input:.3}J"),
    }
}
