use std::ffi::{CStr, c_int, c_uint};

use sensors_sys::{SENSORS_MODE_R, SENSORS_MODE_W, sensors_chip_name, sensors_feature, sensors_feature_type::Type as FeatureType, sensors_get_all_subfeatures, sensors_get_label, sensors_get_subfeature, sensors_get_value, sensors_set_value, sensors_subfeature, sensors_subfeature_type::Type as SubfeatureType};

use super::{LibSensors, chips::Chip, utils::ptr_to_ref, error::{Result, Error}};

/// Get a subfeature by its index.
/// The caller must guarantee that the lifetime of its parameters is at least the lifetime of the sourcing libsensors module
// FIXME: Enforce lifetime to match LibSensors lifetime!
//  (this probably needs to be an instance method for that to work)
unsafe fn subfeature_by_index<'lm>(
    feature: &'lm sensors_feature,
    chip: &'lm sensors_chip_name,
    libsensors: &'lm LibSensors,
    index: &mut c_int
) -> Option<Subfeature<'lm>> {
    // SAFETY: Converting chip and feature to raw pointers is fine, as they must outlive this function
    //  (else Rust would reject the call).
    //  We also know for sure that the function won't modify the passed values, because both name and feature are const parameters.
    let subfeature_raw = unsafe { sensors_get_all_subfeatures(chip, feature, index) };
    // SAFETY: subfeatures are yet again stored by libsensors internally, so they are valid for the lifetime of our libsensors module.
    unsafe { ptr_to_ref(subfeature_raw) }
        .expect("Subfeature is not aligned :(")
        .map(|s| Subfeature::from_raw(s, chip, libsensors))
}

#[derive(Clone, Debug)]
pub struct Feature<'lm> {
    pub name: &'lm CStr,
    pub number: c_int,
    pub type_: FeatureType,
    /// used internally, this is an offset for get_subfeature
    subfeature_start_index: c_int,
    raw: &'lm sensors_feature,
    /// This reference ensures that Feature can only live as long as the LibSensors instance
    libsensors: &'lm LibSensors
}
impl<'lm> Feature<'lm> {
    pub(super) fn from_raw(raw: &'lm sensors_feature, libsensors: &'lm LibSensors) -> Self {
        Feature {
            // SAFETY: (todo. see Chips::from_raw)
            name: unsafe { CStr::from_ptr(raw.name) },
            number: raw.number,
            type_: raw.type_,
            subfeature_start_index: raw.first_subfeature,
            raw,
            libsensors
        }
    }

    pub fn get_subfeature_by_type(&self, chip: &Chip<'lm>, type_: SubfeatureType) -> Option<Subfeature<'lm>> {
        // SAFETY: Both chip.raw and self.raw are valid for the 'lm lifetime (because they are stored internally by libsensors)
        //  type_ is an alias to c_uint so it has no safety issues at all.
        let subfeature_raw = unsafe { sensors_get_subfeature(chip.raw, self.raw, type_) };
        // SAFETY: subfeatures are also stored by libsensors, so are necessarily valid for the lifetime of 'lm.
        unsafe { ptr_to_ref::<'lm>(subfeature_raw) }
            .expect("Subfeature is not aligned :(")
            .map(|f| Subfeature::from_raw(f, chip.raw, self.libsensors))
    }

    pub fn get_subfeature(&self, chip: &Chip<'lm>, index: c_int) -> Option<Subfeature<'lm>> {
        // SAFETY: self cannot outlive its lifetime 'lm. The same is enforced on chip.
        unsafe { subfeature_by_index(self.raw, chip.raw, self.libsensors, &mut (index + self.subfeature_start_index)) }
    }

    pub fn get_subfeatures(&self, chip: &Chip<'lm>) -> SubfeatureIterator<'lm> {
        SubfeatureIterator::new(chip.raw, self.raw, self.subfeature_start_index, self.libsensors)
    }

    pub fn get_name(&self) -> std::result::Result<String, ()> {
        // sensors_get_label returns a manually allocated char*.
        // freeing values allocated in an FFI is an impossible problem in Rust.
        unimplemented!("no implementation for Feature::get_name (would be: sensors_get_label)")
    }
}

pub struct SubfeatureIterator<'lm> {
    chip: &'lm sensors_chip_name,
    feature_raw: &'lm sensors_feature,
    index: c_int,
    libsensors: &'lm LibSensors
}
impl<'lm> SubfeatureIterator<'lm> {
    fn new(
        chip: &'lm sensors_chip_name,
        feature: &'lm sensors_feature,
        start_index: c_int,
        libsensors: &'lm LibSensors
    ) -> Self {
        SubfeatureIterator {
            chip,
            feature_raw: feature,
            index: start_index,
            libsensors
        }
    }
}
impl<'lm> Clone for SubfeatureIterator<'lm> {
    fn clone(&self) -> Self {
        SubfeatureIterator {
            chip: self.chip,
            feature_raw: self.feature_raw,
            index: self.index,
            libsensors: self.libsensors
        }
    }
}
impl<'lm> Iterator for SubfeatureIterator<'lm> {
    type Item = Subfeature<'lm>;

    fn next(&mut self) -> Option<Self::Item> {
        // SAFETY: feature_raw and chip are bound to the parameterized lifetime 'lm.
        // subfeature_by_index already increments index for us (as well as setting it to the first valid value)
        //  (yes indices appear to be stored per-chip, not per-feature)
        unsafe { subfeature_by_index(self.feature_raw, self.chip, self.libsensors, &mut self.index) }
    }
}

#[derive(Clone, Debug)]
pub struct Subfeature<'lm> {
    pub name: &'lm CStr,
    pub number: c_int,
    pub type_: SubfeatureType,
    /// ?
    pub mapping: c_int,
    /// ?
    pub flags: c_uint,
    #[allow(unused)]
    // libsensors reference is a marker to enforce lifetimes. It is important, even though left unused
    libsensors: &'lm LibSensors,
    chip: &'lm sensors_chip_name
}
impl<'lm> Subfeature<'lm> {
    fn from_raw(raw: &'lm sensors_subfeature, chip: &'lm sensors_chip_name, libsensors: &'lm LibSensors) -> Self {
        Subfeature {
            name: unsafe { CStr::from_ptr(raw.name) },
            number: raw.number,
            type_: raw.type_,
            mapping: raw.mapping,
            flags: raw.flags,
            libsensors,
            chip
        }
    }

    pub fn get_value(&self) -> Result<f64> {
        let mut out = 0f64;
        // SAFETY: self.chip is valid at this point (it's a reference after all)
        //  sensors_get_value also doesn't store the pointer in any way
        //  and neither does it store its value parameter.
        Error::convert_cint(
            unsafe {
                sensors_get_value(
                    self.chip,
                    self.number,
                    &mut out
                )
            }
        ).map(|_| out)
    }

    pub fn set_value(&self, value: f64) -> Result<()> {
        Error::convert_cint(unsafe {
            sensors_set_value(
                self.chip,
                self.number,
                value
            )
        }).map(|_| ())
    }

    pub fn can_get(&self) -> bool { 
        // sys-sensors messed up their typing, I can't believe this
        self.flags & (SENSORS_MODE_R as u32) != 0
    }

    pub fn can_set(&self) -> bool {
        self.flags & (SENSORS_MODE_W as u32) != 0
    }
}

/// Feature-independent enum for commonly used subtypes
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericSubfeature {
    Input,
    Min,
    Max,
}
impl GenericSubfeature {
    pub fn to_primitive(self, feature_type: FeatureType) -> Option<SubfeatureType> {
        use sensors_sys::sensors_feature_type::*;
        use sensors_sys::sensors_subfeature_type::*;
        Some(match self {
            // can you see me doing this for every possible subfeature?
            // no. no you cannot. because i will not.
            Self::Input => {
                match feature_type {
                    SENSORS_FEATURE_IN => SENSORS_SUBFEATURE_IN_INPUT,
                    SENSORS_FEATURE_FAN => SENSORS_SUBFEATURE_FAN_INPUT,
                    SENSORS_FEATURE_TEMP => SENSORS_SUBFEATURE_FAN_INPUT,
                    SENSORS_FEATURE_POWER => SENSORS_SUBFEATURE_POWER_INPUT,
                    SENSORS_FEATURE_ENERGY => SENSORS_SUBFEATURE_ENERGY_INPUT,
                    SENSORS_FEATURE_CURR => SENSORS_SUBFEATURE_CURR_INPUT,
                    SENSORS_FEATURE_HUMIDITY => SENSORS_SUBFEATURE_HUMIDITY_INPUT,
                    _ => return None
                }
            },
            Self::Min => {
                match feature_type {
                    SENSORS_FEATURE_IN => SENSORS_SUBFEATURE_IN_MIN,
                    SENSORS_FEATURE_FAN => SENSORS_SUBFEATURE_FAN_MIN,
                    SENSORS_FEATURE_TEMP => SENSORS_SUBFEATURE_TEMP_MIN,
                    SENSORS_FEATURE_POWER => SENSORS_SUBFEATURE_POWER_MIN,
                    SENSORS_FEATURE_CURR => SENSORS_SUBFEATURE_CURR_MIN,
                    _ => return None
                }
            },
            Self::Max => {
                match feature_type {
                    SENSORS_FEATURE_IN => SENSORS_SUBFEATURE_IN_MAX,
                    SENSORS_FEATURE_FAN => SENSORS_SUBFEATURE_FAN_MAX,
                    SENSORS_FEATURE_TEMP => SENSORS_SUBFEATURE_TEMP_MAX,
                    SENSORS_FEATURE_POWER => SENSORS_SUBFEATURE_POWER_MAX,
                    SENSORS_FEATURE_CURR => SENSORS_SUBFEATURE_CURR_MAX,
                    _ => return None
                }
            },
        })
    }
}