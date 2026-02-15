use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use crate::controllers::FanController;

/// Highly generic guard around a collection of FanControllers.
/// When an instance of this type goes out of scope,
/// it attempts to return all fans contained within it to automatic operation.
///
/// This should be the preferred method to ensure fans return to automatic operation,
/// because use of [`Drop`] guarantees that -- barring I/O errors --
/// affected fans will be automated before exiting a function or program,
/// even if it panics.
pub struct AutoGuard<'a, T, R, C>
where
    T: FanController + ?Sized,
    R: AsMut<T>,
    C: AsMut<[R]>,
{
    controllers: C,
    _phantom: PhantomData<&'a mut T>,
    _phantom2: PhantomData<&'a mut R>,
}

impl<'a, T, C, R> AutoGuard<'a, T, R, C>
where
    T: FanController + ?Sized,
    R: AsMut<T>,
    C: AsMut<[R]>,
{
    pub fn wrap(controllers: C) -> Self {
        AutoGuard {
            controllers,
            _phantom: PhantomData,
            _phantom2: PhantomData,
        }
    }

    #[deprecated(note = "AutoGuard::take remains unimplemented and should not be used")]
    pub fn take(self) -> C {
        // the nature of take requires us to somehow fix a drop-check issue,
        // wherein we want to drop this value and _then_ move out one of its fields.
        //
        // However, the existence of take is quite important for general purposes,
        // so I have chosen to leave it unimplemented here as a declaration of intent.
        unimplemented!("take is unimplemented as it is... complicated")
    }
}
impl<'a, T, C, R> From<C> for AutoGuard<'a, T, R, C>
where
    T: FanController + ?Sized,
    R: AsMut<T>,
    C: AsMut<[R]>,
{
    fn from(value: C) -> Self {
        Self::wrap(value)
    }
}

impl<'a, T, R, C> Drop for AutoGuard<'a, T, R, C>
where
    T: FanController + ?Sized,
    R: AsMut<T>,
    C: AsMut<[R]>,
{
    fn drop(&mut self) {
        let _ = crate::utils::return_to_auto(self.controllers.as_mut());
    }
}

impl<'a, T, R, C> AsRef<C> for AutoGuard<'a, T, R, C>
where
    T: FanController + ?Sized,
    R: AsMut<T>,
    C: AsMut<[R]>,
{
    fn as_ref(&self) -> &C {
        &self.controllers
    }
}
impl<'a, T, R, C> AsMut<C> for AutoGuard<'a, T, R, C>
where
    T: FanController + ?Sized,
    R: AsMut<T>,
    C: AsMut<[R]>,
{
    fn as_mut(&mut self) -> &mut C {
        &mut self.controllers
    }
}
impl<'a, T, R, C> Deref for AutoGuard<'a, T, R, C>
where
    T: FanController + ?Sized,
    R: AsMut<T>,
    C: AsMut<[R]>,
{
    type Target = C;

    fn deref(&self) -> &Self::Target {
        &self.controllers
    }
}
impl<'a, T, R, C> DerefMut for AutoGuard<'a, T, R, C>
where
    T: FanController + ?Sized,
    R: AsMut<T>,
    C: AsMut<[R]>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.controllers
    }
}
