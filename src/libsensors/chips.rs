use std::{ffi::{CStr, c_int, c_short}, fmt::Display, iter::FusedIterator};

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

#[derive(Clone, Debug)]
pub struct Chip<'lm> {
    pub prefix: &'lm str,
    pub bus: Bus,
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
            prefix: unsafe { CStr::from_ptr(raw.prefix) }.to_str().expect("TODO: Error handling for failed to_str conversion on Chip.prefix"),
            bus: Bus::try_from(raw.bus).expect("TODO: Better error handling for failed bus parsing on Chip.bus"),
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
impl<'lm> Display for Chip<'lm> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}-", self.prefix, self.bus)?;
        let mut past_prefix_zeroes = false;
        for byte in self.addr.to_be_bytes() {
            if byte != 0 || past_prefix_zeroes {
                past_prefix_zeroes = true;
                write!(f, "{byte:02x}")?;
            }
        }
        Ok(())
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

#[derive(Clone, Debug)]
pub struct Bus {
    pub type_: BusType,
    pub nr: c_short
}
impl TryFrom<sensors_bus_id> for Bus {
    type Error = strum::ParseError;

    fn try_from(value: sensors_bus_id) -> Result<Self, Self::Error> {
        BusType::from_repr(value.type_)
            .ok_or(strum::ParseError::VariantNotFound)
            .map(|bus_type| {
                Bus {
                    type_: bus_type,
                    nr: value.nr
                }
            })
    }
}
impl Display for Bus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.type_, self.nr)
    }
}

#[repr(i16)]
#[derive(Clone, Copy, Debug, strum::FromRepr)]
pub enum BusType {
    I2C = 0,
    ISA = 1,
    PCI = 2,
    SPI = 3,
    VIRTUAL = 4,
    ACPI = 5,
    HID = 6,
    MDIO = 7,
    SCSI = 8
}
impl BusType {
    pub fn str_repr(&self) -> &'static str {
        use self::BusType::*;
        match self {
            I2C => "i2c",
            ISA => "isa",
            PCI => "pci",
            SPI => "spi",
            VIRTUAL => "virt",
            ACPI => "acpi",
            HID => "hid",
            MDIO => "mdio",
            SCSI => "scsi"
        }
    }
}
impl Display for BusType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.str_repr())
    }
}