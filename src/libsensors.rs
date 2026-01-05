use std::{iter::FusedIterator, sync::atomic::{AtomicBool, Ordering as MemOrdering}};

use sensors_sys::{sensors_cleanup, sensors_get_detected_chips, sensors_init};

use crate::libsensors::{chips::Chip, utils::ptr_to_ref};

use self::error::{Error, Result};

pub mod error;
pub mod chips;
pub mod features;
mod utils;

static LIBSENSORS_DOES_NOT_EXIST: AtomicBool = AtomicBool::new(true);

/// A handle to an initialized libsensors environment.
/// Note that only one of these may exist at the same time during the lifetime of a program!
/// libsensors also makes no claims as to thread safety, so creating two instances in different threads is also forbidden!
#[derive(Debug)]
pub struct LibSensors {

}
impl LibSensors {
    /// Initialises Libsensors and returns a hanlde to it.
    /// 
    /// Note that no two instances of this struct can exist at the same time.
    /// Trying to create an instance while another exists will raise an error.
    /// Furthermore, if two threads race (one dropping an instance, another creating it),
    /// there is no guarantee that the second thread will not encounter a duplication error,
    /// even if their timings were perfect.
    /// If you do this, you should create proper synchronisation around the threads.
    pub fn init() -> Result<Self> {
        // Acquire/Release is necessary here.
        // Acquire guarantees nobody stores, while we're reading.
        // Release guarantees nobody reads, while we're storing.
        if LIBSENSORS_DOES_NOT_EXIST.fetch_and(false, MemOrdering::AcqRel) {
            // SAFETY: sensors_init can accept nullptr, in which case it uses the default configuration.
            //  If we ever decide to allow custom configurations, we may need to cast an Option<File> accordingly.
            //  sensors_init also returns error codes, but handles the cleanup itself.
            Error::convert_cint(unsafe { sensors_init(std::ptr::null_mut()) })
                .map(|_| LibSensors {  })
                // fetch_and above asserts that no two threads can be in this side of the if-stament at the same time.
                // Therefore we have guarantee, that at this point, LIBSENSORS_DOES_NOT_EXIST is false, so we can simply set it true.
                // (Using Relaxed here is fine, as we don't guarantee that this call succeeds, even if no LibSensors object exists)
                .inspect_err(|_| LIBSENSORS_DOES_NOT_EXIST.store(true, MemOrdering::Relaxed))
        } else {
            Err(todo!("Implement an error format for already-initialised"))
        }
    }

    /// Gets a chip (in groupie terms: an adapter) at the given index in libsensors' storage.
    /// 
    /// Returns a reference to that chip's name, or None if index is >= the chip count.
    /// If you need multiple chips, use [`LibSensors::get_chips`]
    // libsensors will increment the index itself, but no way am I trusting that!
    // lifetime is explicit here, because we really don't want to mess up this lifetime should we change the function
    pub fn get_chip<'lm>(&'lm self, mut index: std::ffi::c_int) -> Option<Chip<'lm>> {
        // SAFETY: allowed to pass a nullptr here, as that will assume no match.
        //  sensors_get_detected_chips returns a pointer to an internal data structure,
        //  which will live as long as sensors_cleanup is not called (that is: as long as self lives).
        //  If only we had some kind of feature that could check the lifetime of references or something...
        let result_raw = unsafe { sensors_get_detected_chips(std::ptr::null_mut(), &mut index) };
        // SAFETY: We know this pointer points to initialized memory,
        //  - it can't deallocate before its reference becomes invalid
        //     (because of the lifetime specified above)
        //  - it probably? surely? definitely? won't be mutated by some evil libsensors shenanigans
        unsafe { ptr_to_ref::<'lm>(result_raw) }
            .expect("chip name is not properly aligned! How could you do this to me lm-sensors???")
            .map(|c| Chip::from_raw(c, self))
    }

    /// Get an iterator over all chips known to libsensors.
    pub fn get_chips<'a>(&'a self) -> ChipIterator<'a> {
        ChipIterator::new(self)
    }
}
impl Drop for LibSensors {
    fn drop(&mut self) {
        // SAFETY: Luckily sensors_cleanup is void, so there are no errors to handle
        //  sensors_cleanup also frees any memory allocated by the C-code,
        //  so it has basically the same safety requirements as Vec
        unsafe { sensors_cleanup(); }
        // Since LibSensors::init asserts that only one instance of LibSensors can exist at once,
        // it inevitably asserts that only one drop-call can be issued at about the same time.
        // This means storing true at this point is fine, as is a Relaxed memory order.
        LIBSENSORS_DOES_NOT_EXIST.store(true, MemOrdering::Relaxed)
    }
}

pub struct ChipIterator<'lm> {
    sensors: &'lm LibSensors,
    index: i32
}
impl<'lm> ChipIterator<'lm> {
    fn new(sensors: &'lm LibSensors) -> Self {
        ChipIterator { sensors, index: 0 }
    }
}
impl<'lm> Clone for ChipIterator<'lm> {
    // this impl could be derived, but this way is much clearer
    /// Clones the iterator, by creating a new iterator that starts at the current index of this one
    fn clone(&self) -> Self {
        Self { sensors: self.sensors, index: self.index }
    }
}
impl<'lm> Iterator for ChipIterator<'lm> {
    type Item = Chip<'lm>;

    fn next(&mut self) -> Option<Self::Item> {
        let val = self.sensors.get_chip(self.index);
        if val.is_some() {
            self.index += 1;
        }
        val
    }
}
impl<'lm> FusedIterator for ChipIterator<'lm> { }