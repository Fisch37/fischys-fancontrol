use std::{ffi::{CStr, c_int}, iter::FusedIterator};

use sensors_sys::{sensors_bus_id, sensors_chip_name, sensors_get_features};

use super::{LibSensors, features::Feature, utils::ptr_to_ref};

fn get_feature_raw<'lm>(
    raw_chip: &'lm sensors_chip_name,
    libsensors: &'lm LibSensors,
    mut index: c_int
) -> Option<Feature<'lm>> {
    // SAFETY: sensors_get_features is pretty safe tbh.
    //  raw_chip points to a valid sensor, so long as LibSensors lives (enforced by the lifetime & rust safety in general)
    //  the index can be mutated as much as they want, because we don't do anything with it anyway.
    //  the return value is also valid for the lifetime of 'lm, so we're free here as well.
    let feature_raw = unsafe { sensors_get_features(raw_chip, &mut index) };
    // SAFETY: As mentioned above, feature_raw is valid for the entire lifetime of 'lm.
    //  It also points to exactly one valid feature and we mustn't assume any more.
    //  That's fine, because we don't. Our return type is a reference &'lm, which enforces everything we need.
    unsafe { ptr_to_ref::<'lm>(feature_raw) }
        .expect("Feature is not aligned! Whyyy?")
        .map(|f| Feature::from_raw(f, libsensors))
}

#[derive(Clone)]
pub struct Chip<'lm> {
    pub prefix: &'lm CStr,
    pub bus: sensors_bus_id,
    pub addr: c_int,
    pub path: &'lm CStr,

    pub(crate) raw: &'lm sensors_chip_name,
    /// Carrying this reference ensures lifetime bounds are respected
    libsensors: &'lm LibSensors
}
impl<'lm> Chip<'lm> {
    pub(super) fn from_raw(raw: &'lm sensors_chip_name, libsensors: &'lm LibSensors) -> Self {
        Chip {
            // SAFETY: the data passed from C must be valid (people are supposed to use it, no?)
            //  therefore, it is a valid C-String, therefore this is safe.
            //  The data will also exist until the next sensors_cleanup call, which means the 'lm lifetime passed above.
            //  (I'm still not sure about immutability though)
            prefix: unsafe { CStr::from_ptr(raw.prefix) },
            bus: raw.bus,
            addr: raw.addr,
            // SAFETY: see above.
            path: unsafe { CStr::from_ptr(raw.path) },
            raw,
            libsensors
        }
    }

    /// Gets a reference to a single feature (in groupie terms: sensor) of this chip.
    /// If index >= the amount of features present on this chip, returns None.
    /// 
    /// Hint: Use [`Chip::get_features`] if you need to access multiple features.
    pub fn get_feature(&self, index: c_int) -> Option<Feature<'lm>> {
        get_feature_raw(self.raw, self.libsensors, index)
    }

    /// Gets an iterator over all features (sensors) associated with this chip.
    pub fn get_features(&self) -> FeatureIterator<'lm> {
        // I'm a bit annoyed, at re-exposing raw here,
        // but not doing so would bind me to &self's lifetime unnecessarily.
        FeatureIterator::new(self.raw, self.libsensors)
    }
}

pub struct FeatureIterator<'lm> {
    raw_chip: &'lm sensors_chip_name,
    index: c_int,
    libsensors: &'lm LibSensors
}
impl<'lm> FeatureIterator<'lm> {
    fn new(raw_chip: &'lm sensors_chip_name, libsensors: &'lm LibSensors) -> Self {
        FeatureIterator { raw_chip, index: 0, libsensors }
    }
}
impl<'lm> Clone for FeatureIterator<'lm> {
    // implementing clone explicitly, because it's less error prone
    /// Clones this iterator, by creating a new one at the current position.
    fn clone(&self) -> Self {
        Self { raw_chip: self.raw_chip, index: self.index, libsensors: self.libsensors }
    }
}
impl<'lm> Iterator for FeatureIterator<'lm> {
    type Item = Feature<'lm>;

    fn next(&mut self) -> Option<Self::Item> {
        let val = get_feature_raw(self.raw_chip, self.libsensors, self.index);
        if val.is_some() {
            self.index += 1;
        }
        val
    }
}
impl<'lm> FusedIterator for FeatureIterator<'lm> { }