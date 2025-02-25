use derive_more::derive::{Deref, DerefMut};
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin};

/// A type that wraps another with a signature of it.
#[derive(Debug, Clone, Serialize, Deserialize, Deref, DerefMut)]
pub struct Signed<T, S> {
    /// The inner type
    #[deref]
    #[deref_mut]
    #[serde(flatten)]
    inner: T,
    /// The signature of the wrapped field
    signature: S,
}

impl<T, S> Signed<T, S> {
    /// Create a new signed object.
    #[inline]
    pub fn new(inner: T, signature: S) -> Self {
        Self { inner, signature }
    }

    /// Get the inner object.
    #[inline]
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Get the inner mut object.
    #[inline]
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Get the signature.
    #[inline]
    pub fn signature(&self) -> &S {
        &self.signature
    }
}

/// Types that can be signed.
pub trait IntoSigned<S> {
    /// Wrap the type with a signature.
    fn into_signed(self, signature: S) -> Signed<Self, S>
    where
        Self: Sized;
}

impl<T, S> IntoSigned<S> for T {
    #[inline]
    fn into_signed(self, signature: S) -> Signed<Self, S> {
        Signed {
            inner: self,
            signature,
        }
    }
}

/// A type with an associated ID.
#[derive(Debug, Clone, Serialize, Deserialize, Deref, DerefMut)]
pub struct Identified<T, I: Copy> {
    /// The inner type
    #[deref]
    #[deref_mut]
    #[serde(flatten)]
    inner: T,
    /// The ID of the object
    id: I,
}

impl<T, I: Copy> Identified<T, I> {
    /// Create a new identified object.
    #[inline]
    pub fn new(inner: T, id: I) -> Self {
        Self { inner, id }
    }

    /// Get the inner object.
    #[inline]
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Get the inner object.
    #[inline]
    pub fn into_inner(self) -> T {
        self.inner
    }

    /// Get the inner mut object.
    #[inline]
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Get the ID.
    #[inline]
    pub fn id(&self) -> I {
        self.id
    }
}

/// Types that can be wrapped with identified.
pub trait IntoIdentified<I>
where
    I: Copy,
{
    /// Wrap the type with an ID.
    fn into_identified(self, id: I) -> Identified<Self, I>
    where
        Self: Sized;
}

impl<T, I: Copy> IntoIdentified<I> for T {
    #[inline]
    fn into_identified(self, id: I) -> Identified<Self, I> {
        Identified { inner: self, id }
    }
}

/// A `Result` type where both `T` and `E` are `Identified`.
pub trait IdentifiedError<I>
where
    I: Copy,
{
    /// Returns the ID of the object, whether it's an error or not.
    fn id(&self) -> I;
}

impl<T, E, I> IdentifiedError<I> for Result<Identified<T, I>, Identified<E, I>>
where
    I: Copy,
{
    fn id(&self) -> I {
        match self {
            Ok(t) => t.id(),
            Err(e) => e.id(),
        }
    }
}

/// Generic shutdown signal future.
pub type ShutdownSignal = Pin<Box<dyn Future<Output = ()> + Send>>;
