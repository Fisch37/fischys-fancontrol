

/// Converts a raw pointer into a safe reference, valid for a given lifetime (inferred from usage).
/// 
/// The caller must guarantee that this pointer is actually valid for the given lifetime
/// and that it is either null or that the data it points to is valid for the given type of the resulting reference.
/// The caller must also guarantee that the data behind ptr is not mutated for the entire lifetime of the reference.
/// 
/// Returns an Err if the pointer is not aligned, an Ok(None) if the pointer is null,
/// and an Ok(Some(ref)) with the correct reference.
pub unsafe fn ptr_to_ref<'a, T>(ptr: *const T) -> Result<Option<&'a T>, ()> {
    if ptr.is_null() {
        Ok(None)
    } else if !ptr.is_aligned() {
        Err(())
    } else {
        // Caller has guaranteed that the reference is valid for the lifetime 'a
        // and contains valid data. 
        Ok(Some(unsafe { &*ptr }))
    }
}