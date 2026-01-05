use std::{ffi::c_uint, rc::Rc};

use log::{debug, warn};

use crate::{GlobalContext, groupie::{Adapter, QueryResult, SensorData, SensorKind}, libsensors::features::{GenericSubfeature, Subfeature}};

pub fn query_sensors(state: &mut QueryResult, context: &GlobalContext) -> Result<(), Box<dyn std::error::Error>> {
    let libsensors = context.get_libsensors();
    for chip in libsensors.get_chips() {
        let adapter = Rc::new(Adapter {
            key: chip.to_string(),
            name: chip.prefix.to_owned()
        });

        for feature in chip.get_features() {
            // black magic! the feature type left 8 bits is always the input subfeature (for that feature type).
            // How do I know this? Divination! (checking the sensors.h file of lm-sensors manually)
            let input_subfeature_type: c_uint = feature.type_ << 8;
            let input_value = match feature.get_subfeature_by_type(&chip, input_subfeature_type) {
                Some(subfeature) => {
                    match subfeature.get_value() {
                        Ok(x) => x,
                        Err(e) => {
                            warn!(
                                "Input subfeature for {:?} on chip {:?} is not readable! Error code: {e}",
                                feature,
                                chip
                            );
                            continue;
                        }
                    }
                },
                None => {
                    debug!(
                        "Libsensors feature {:?} on chip {:?} does not have an input type. Skipping it",
                        feature,
                        chip
                    );
                    continue;
                }
            };
            let kind = sensor_kind_from_feature_type(feature.type_);
            match kind {
                Some(x) => {
                    if let Err(sensor) = state.add(SensorData {
                        kind: x,
                        name: feature.name.to_str()?.to_owned(),
                        input: input_value,
                        min: {
                            GenericSubfeature::Min.to_primitive(feature.type_)
                                .and_then(|subfeature_type| try_read_subfeature(feature.get_subfeature_by_type(&chip, subfeature_type)))
                                .unwrap_or(-f64::INFINITY)
                        },
                        max: {
                            GenericSubfeature::Max.to_primitive(feature.type_)
                                .and_then(|subfeature_type| try_read_subfeature(feature.get_subfeature_by_type(&chip, subfeature_type)))
                                .unwrap_or(f64::INFINITY)
                        },
                        adapter: adapter.clone()
                    }) {
                        warn!("Tried to add duplicate sensor {sensor:?}")
                    }
                },
                None => {
                    warn!("Skipping sensor of unknown feature type {}", feature.type_);
                }
            }
        }
    }
    Ok(())
}

fn sensor_kind_from_feature_type(feature_type: c_uint) -> Option<SensorKind> {
    use sensors_sys::sensors_feature_type::*;
    Some(match feature_type {
        SENSORS_FEATURE_CURR => SensorKind::Current,
        SENSORS_FEATURE_ENERGY => SensorKind::Energy,
        SENSORS_FEATURE_FAN => SensorKind::Fan,
        SENSORS_FEATURE_IN => SensorKind::Voltmeter,
        SENSORS_FEATURE_POWER => SensorKind::Power,
        SENSORS_FEATURE_TEMP => SensorKind::Temperature,
        _ => return None
    })
}

fn try_read_subfeature(subfeature: Option<Subfeature>) -> Option<f64> {
    subfeature
        .and_then(|subfeature| {
            let val = subfeature.get_value().ok();
            if val.is_none() {
                // very quiet
                debug!("Failed to grab subfeature {subfeature:?}. Failing quietly.");
            }
            val
        })
}