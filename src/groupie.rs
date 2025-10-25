use std::{cmp::Ordering, error::Error, hash::Hash, ops::Index, rc::Rc, slice::SliceIndex};

use strum::{EnumCount, EnumIter};

mod nvidia;
mod lm_sensors;

#[derive(Debug, Eq)]
pub struct Adapter {
    pub key: String,
    pub name: String
}
impl PartialEq for Adapter {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl PartialOrd for Adapter {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Adapter {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}

#[derive(Debug)]
pub struct SensorData {
    pub kind: SensorKind,
    pub name: String,
    pub input: f64,
    pub min: f64,
    pub max: f64,
    pub adapter: Rc<Adapter>
}
impl PartialEq for SensorData {
    fn eq(&self, other: &Self) -> bool {
        self.adapter == other.adapter && self.name == other.name
    }
}
impl Eq for SensorData { }
impl PartialOrd for SensorData {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for SensorData {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cmp_wireframe(&other.adapter.key, &other.name)
    }
}
impl SensorData {
    fn cmp_wireframe(&self, adapter: &String, name: &String) -> Ordering {
        match self.adapter.key.cmp(adapter) {
            Ordering::Equal => self.name.cmp(name),
            a => a
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, Hash)]
#[derive(PartialEq, Eq, PartialOrd, Ord)]
#[derive(EnumCount, EnumIter)]
pub enum SensorKind {
    Temperature, Fan, Voltmeter, Beep
}
impl SensorKind {
    pub fn from_string(name: &str) -> Option<SensorKind> {
        if name.starts_with("temp") {
            return Some(SensorKind::Temperature);
        } else if name.starts_with("fan") {
            return Some(SensorKind::Fan);
        } else if name.starts_with("in") {
            return Some(SensorKind::Voltmeter);
        } else if name.starts_with("beep") {
            return Some(SensorKind::Beep)
        } else {
            return None;
        }
    }
}

pub struct QueryResult {
    internal: [SensorStorage; SensorKind::COUNT]
}
impl QueryResult {
    pub fn new() -> QueryResult {
        // const block is once per array element. Don't ask me why, but I tested and that's how it works
        return QueryResult { internal: [const { SensorStorage::new() }; SensorKind::COUNT] };
    }

    pub fn get_of_kind<'a>(&'a self, kind: SensorKind) -> &'a SensorStorage {
        &self.internal[kind as usize]
    }

    fn get_of_kind_mut<'a>(&'a mut self, kind: SensorKind) -> &'a mut SensorStorage {
        &mut self.internal[kind as usize]
    }

    // Returns Err if an equal value is already present
    pub fn add(&mut self, data: SensorData) -> Result<(), SensorData> {
        self.get_of_kind_mut(data.kind).add(data)
    }

    fn clear(&mut self) {
        for store in &mut self.internal {
            store.clear();
        }
    }

    fn shrink_to_fit(&mut self) {
        for store in &mut self.internal {
            store.shrink_to_fit();
        }
    }
}
#[derive(Debug)]
pub struct SensorStorage {
    internal: Vec<SensorData>
}
impl<'a> IntoIterator for &'a SensorStorage {
    type Item = &'a SensorData;

    type IntoIter = std::slice::Iter<'a, SensorData>;

    fn into_iter(self) -> Self::IntoIter {
        self.internal.as_slice().into_iter()
    }
}
impl<I: SliceIndex<[SensorData], Output = SensorData>> Index<I> for SensorStorage {
    type Output = SensorData;

    fn index(&self, index: I) -> &Self::Output {
        self.internal.index(index)
    }
}
impl<'a> From<&'a SensorStorage> for &'a [SensorData] {
    fn from(value: &'a SensorStorage) -> Self {
        return &value.internal.as_slice()
    }
}
impl SensorStorage {
    pub const fn new() -> Self {
        // no preset capacity as the required capacity is unknown before insertion
        // SensorStorage is also expected to be reused frequently, so the actual impact is minimal
        SensorStorage { internal: Vec::new() }
    }

    pub fn get(&self, adapter: &String, name: &String) -> Option<&SensorData> {
        match self.internal.binary_search_by(|sensor| sensor.cmp_wireframe(adapter, name)) {
            Ok(i) => Some(&self.internal[i]),
            Err(_) => None
        }
    }

    fn add(&mut self, data: SensorData) -> Result<(), SensorData> {
        match self.internal.binary_search(&data) {
            Ok(_) => Err(data),
            Err(i) => {
                self.internal.insert(i, data);
                Ok(())
            }
        }
    }

    fn clear(&mut self) {
        self.internal.clear();
    }

    fn shrink_to_fit(&mut self) {
        self.internal.shrink_to_fit();
    }
}

pub fn query_sensors(state: &mut QueryResult) -> Result<(), Box<dyn Error>> {
    state.clear();

    let res = lm_sensors::query_sensors(state)
        .and(nvidia::query_sensors(state));

    // Unfortunately the nature of the data structure makes it impossible to estimate the capacity per category.
    // However! If the QueryResult is reused (as it should be), there is unlikely to be any change in size after the first call.
    state.shrink_to_fit();
    return res;
}