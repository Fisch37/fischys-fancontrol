use std::{rc::Rc, str::Utf8Error};

use libsensors_rs::{Feature, GenericSubfeature, Chip, feature::FeatureType};
use log::{debug, warn};

use crate::{GlobalContext, groupie::{Adapter, QueryResult, SensorData, SensorKind}};

pub fn chip_key(chip: &Chip) -> Result<String, Utf8Error> {
    Ok(format!(
        "{}-{}-0b{:b}@{:X}",
        chip.get_prefix().to_str()?,
        Into::<&'static str>::into(chip.get_bus_id().type_),
        chip.get_bus_id().nr,
        chip.get_address()
    ))
}

pub fn query_sensors(state: &mut QueryResult, context: &GlobalContext) -> Result<(), Box<dyn std::error::Error>> {
    let libsensors = context.get_libsensors();
    for chip in libsensors.get_chips()? {
        let chip = chip?;
        let adapter = Rc::new(Adapter {
            key: chip_key(&chip)?,
            name: chip.get_name()?
                .map(|s| Ok(s.to_owned()))
                .unwrap_or_else(|| chip_key(&chip))?
        });

        for feature in chip.get_features()? {
            let feature = feature?;
            let feature_type = feature.get_type();
            let input_subtype = match GenericSubfeature::Input.to_primitive(feature.get_type()) {
                Some(x) => x,
                None => {
                    debug!("Feature type {feature_type:?} does not have an input subtype (or it is not supported). Skipping it.");
                    continue;
                }
            };
            let input_value = match feature.get_subfeature_by_type(input_subtype)? {
                Some(subfeature) => {
                    match subfeature.get_value() {
                        Ok(x) => x,
                        Err(e) => {
                            warn!("Input subfeature for {feature:?} on chip {chip:?} is not readable! Error code: {e}");
                            continue;
                        }
                    }
                },
                None => {
                    debug!(
                        "Libsensors feature {:?} ({feature:?}) on chip {chip:?} does not have an input type. Skipping it",
                        feature.get_name()
                    );
                    continue;
                }
            };
            let kind = sensor_kind_from_feature_type(feature_type);
            match kind {
                Some(x) => {
                    if let Err(sensor) = state.add(SensorData {
                        kind: x,
                        name: feature.get_label()?,
                        input: input_value,
                        min: try_read_subfeature(
                            &feature,
                            GenericSubfeature::Min,
                            -f64::INFINITY
                        ),
                        max: try_read_subfeature(
                            &feature,
                            GenericSubfeature::Max,
                            f64::INFINITY
                        ),
                        adapter: adapter.clone()
                    }) {
                        warn!("Tried to add duplicate sensor {sensor:?}")
                    }
                },
                None => {
                    warn!("Skipping sensor of unknown feature type {feature_type:?}");
                }
            }
        }
    }
    Ok(())
}

fn sensor_kind_from_feature_type(feature_type: FeatureType) -> Option<SensorKind> {
    use FeatureType::*;
    Some(match feature_type {
        Current => SensorKind::Current,
        Energy => SensorKind::Energy,
        Fan => SensorKind::Fan,
        In => SensorKind::Voltmeter,
        Power => SensorKind::Power,
        Temp => SensorKind::Temperature,
        _ => return None
    })
}

fn try_read_subfeature(feature: &Feature, subfeature_type: GenericSubfeature, default: f64) -> f64 {
    subfeature_type.to_primitive(feature.get_type())
        .and_then(|specific_type| {
            feature.get_subfeature_by_type(specific_type).ok()
                .and_then(|s| s)
                .and_then(|s| s.get_value().ok())
        })
        .unwrap_or_else(|| {
            debug!("Failed to grab subfeature {subfeature_type:?}. Failing quietly");
            default
        })
}
