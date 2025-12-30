use std::ops::Deref;

use strum::IntoEnumIterator;

use crate::{GlobalContext, groupie::{QueryResult, SensorKind, query_sensors}};

fn format_input(kind: SensorKind, input: f64) -> String {
    match kind {
        SensorKind::Temperature => format!("{:>2.1}°C", input),
        SensorKind::Fan => format!("{:>4.0} RPM", input),
        SensorKind::Voltmeter => format!("{:.3}V", input),
        SensorKind::Beep => (if input > 0.0 { "true" } else { "false" }).to_string()
    }
}

pub fn start() {
    let context = GlobalContext::init().unwrap();
    let mut state = QueryResult::new();
    query_sensors(&mut state, &context).unwrap();
    for kind in SensorKind::iter() {
        println!("{:?}:", kind);
        let mut last_adapter = None;
        for sensor in state.get_of_kind(kind).deref() {
            if Some(&*sensor.adapter) != last_adapter {
                println!("{} ({})", sensor.adapter.name, sensor.adapter.key);
            }
            println!("\t{:<15} {} (min {}, max {})",
                sensor.name, format_input(kind, sensor.input),
                format_input(kind, sensor.min), format_input(kind, sensor.max)
            );
            last_adapter = Some(&*sensor.adapter);
        }
    }
}