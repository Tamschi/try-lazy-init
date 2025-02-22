#![doc(html_root_url = "https://docs.rs/try-lazy-init/0.0.2")]
#![warn(clippy::pedantic)]
#![allow(clippy::semicolon_if_nothing_returned)]
#![deny(missing_docs)]

//! A crate for things that are
//! 1) Lazily initialized
//! 2) Expensive to create
//! 3) Immutable after creation
//! 4) Used on multiple threads
//!
//! `Lazy<T>` is better than `Mutex<Option<T>>` because after creation accessing
//! `T` does not require any locking, just a single boolean load with
//! `Ordering::Acquire` (which on x86 is just a compiler barrier, not an actual
//! memory barrier).

#[cfg(doctest)]
pub mod readme {
	doc_comment::doctest!("../README.md");
}

use std::{
	cell::UnsafeCell,
	fmt,
	sync::{
		atomic::{AtomicBool, Ordering},
		Mutex,
	},
};

#[derive(Clone)]
enum ThisOrThat<T, U> {
	This(T),
	That(U),
}

/// `LazyTransform<T, U>` is a synchronized holder type, that holds a value of
/// type T until it is lazily converted into a value of type U.
pub struct LazyTransform<T, U> {
	initialized: AtomicBool,
	lock: Mutex<()>,
	value: UnsafeCell<Option<ThisOrThat<T, U>>>,
}

// Implementation details.
impl<T, U> LazyTransform<T, U> {
	fn extract(&self) -> Option<&U> {
		// Make sure we're initialized first!
		match unsafe { (*self.value.get()).as_ref() } {
			None => None,
			Some(&ThisOrThat::This(_)) => panic!(), // Should already be initialized!
			Some(&ThisOrThat::That(ref that)) => Some(that),
		}
	}
}

// Public API.
impl<T, U> LazyTransform<T, U> {
	/// Construct a new, untransformed `LazyTransform<T, U>` with an argument of
	/// type T.
	pub fn new(t: T) -> LazyTransform<T, U> {
		LazyTransform {
			initialized: AtomicBool::new(false),
			lock: Mutex::new(()),
			value: UnsafeCell::new(Some(ThisOrThat::This(t))),
		}
	}

	/// Unwrap the contained value, returning `Ok(U)` if the `LazyTransform<T, U>` has been transformed.
	///
	/// # Errors
	///
	/// Iff this instance has not been transformed yet.
	///
	/// # Panics
	///
	/// Iff this instance has been poisoned during transformation.
	pub fn into_inner(self) -> Result<U, T> {
		// We don't need to inspect `self.initialized` since `self` is owned
		// so it is guaranteed that no other threads are accessing its data.
		match self.value.into_inner().unwrap() {
			ThisOrThat::This(t) => Err(t),
			ThisOrThat::That(u) => Ok(u),
		}
	}

	/// Unwrap the contained value, returning `Ok(Ok(U))` iff the `LazyTransform<T, U>` has been transformed.
	///
	/// # Errors
	///
	/// Iff this instance has neither been transformed yet nor poisoned, `Err(Some(T))` is returned.
	///
	/// Iff this instance has been poisoned *by error* during a call to [`.get_or_create_or_poison`](`LazyTransform::get_or_create_or_poison`), `Err(None)` is returned.
	///
	/// # Panics
	///
	/// Iff this instance has been poisoned *by a panic* during transformation.
	pub fn try_into_inner(self) -> Result<U, Option<T>> {
		// We don't need to inspect `self.initialized` since `self` is owned
		// so it is guaranteed that no other threads are accessing its data.
		match self.value.into_inner() {
			None => Err(None),
			Some(ThisOrThat::This(t)) => Err(Some(t)),
			Some(ThisOrThat::That(u)) => Ok(u),
		}
	}
}

// Public API.
impl<T, U> LazyTransform<T, U> {
	/// Get a reference to the transformed value, invoking `f` to transform it
	/// if the `LazyTransform<T, U>` has yet to be transformed.  It is
	/// guaranteed that if multiple calls to `get_or_create` race, only one
	/// will invoke its closure, and every call will receive a reference to the
	/// newly transformed value.
	///
	/// The closure can only ever be called once, so think carefully about what
	/// transformation you want to apply!
	///
	/// # Panics
	///
	/// This method will panic if the instance has been poisoned during a previous transformation attempt.
	///
	/// The method **may** panic (or deadlock) upon reentrance.
	pub fn get_or_create<F>(&self, f: F) -> &U
	where
		F: FnOnce(T) -> U,
	{
		// In addition to being correct, this pattern is vouched for by Hans Boehm
		// (http://schd.ws/hosted_files/cppcon2016/74/HansWeakAtomics.pdf Page 27)
		if !self.initialized.load(Ordering::Acquire) {
			// We *may* not be initialized. We have to block to be certain.
			let _lock = self.lock.lock().unwrap();
			#[allow(clippy::if_not_else)]
			if !self.initialized.load(Ordering::Relaxed) {
				// Ok, we're definitely uninitialized.
				// Safe to fiddle with the UnsafeCell now, because we're locked,
				// and there can't be any outstanding references.
				let value = unsafe { &mut *self.value.get() };
				let this = match value.take().unwrap() {
					ThisOrThat::This(t) => t,
					ThisOrThat::That(_) => panic!(), // Can't already be initialized!
				};
				*value = Some(ThisOrThat::That(f(this)));
				self.initialized.store(true, Ordering::Release);
			} else {
				// We raced, and someone else initialized us. We can fall
				// through now.
			}
		}

		// We're initialized, our value is immutable, no synchronization needed.
		self.extract().unwrap()
	}

	/// Try to get a reference to the transformed value, invoking a fallible `f` to
	/// transform it if the `LazyTransform<T, U>` has yet to be transformed.
	/// It is guaranteed that if multiple calls to `get_or_create` race, only one
	/// will **successfully** invoke its closure, and every call will receive a
	/// reference to the newly transformed value.
	///
	/// The closure can only ever be successfully called once, so think carefully
	/// about what transformation you want to apply!
	///
	/// # Errors
	///
	/// Iff `f` returns a [`Result::Err`], this error is returned verbatim.
	///
	/// # Panics
	///
	/// This method will panic if the instance has been poisoned during a previous transformation attempt.
	///
	/// The method **may** panic (or deadlock) upon reentrance.
	pub fn try_get_or_create<F, E>(&self, f: F) -> Result<&U, E>
	where
		T: Clone,
		F: FnOnce(T) -> Result<U, E>,
	{
		// In addition to being correct, this pattern is vouched for by Hans Boehm
		// (http://schd.ws/hosted_files/cppcon2016/74/HansWeakAtomics.pdf Page 27)
		#[allow(clippy::if_not_else)]
		if !self.initialized.load(Ordering::Acquire) {
			// We *may* not be initialized. We have to block to be certain.
			let _lock = self.lock.lock().unwrap();
			if !self.initialized.load(Ordering::Relaxed) {
				// Ok, we're definitely uninitialized.
				// Safe to fiddle with the UnsafeCell now, because we're locked,
				// and there can't be any outstanding references.
				//
				// However, since this function can return early without poisoning this instance,
				// `self.value` must stay valid until overwritten with `f`'s `Ok`.
				let value = unsafe { &mut *self.value.get() };
				let this = match value.as_ref().unwrap() {
					ThisOrThat::This(t) => t.clone(),
					ThisOrThat::That(_) => panic!(), // Can't already be initialized!
				};
				*value = Some(ThisOrThat::That(f(this)?));
				self.initialized.store(true, Ordering::Release);
			} else {
				// We raced, and someone else initialized us. We can fall
				// through now.
			}
		}

		// We're initialized, our value is immutable, no synchronization needed.
		Ok(self.extract().unwrap())
	}

	/// Try to get a reference to the transformed value, invoking a fallible `f` to
	/// transform it if the `LazyTransform<T, U>` has yet to be transformed.
	/// It is guaranteed that if multiple calls to `get_or_create` race, only one
	/// will invoke its closure, and every call will receive a reference to the
	/// newly transformed value.
	///
	/// The closure can only ever be called once, so think carefully
	/// about what transformation you want to apply!
	///
	/// # Errors
	///
	/// Iff this instance is poisoned, *except by panics*, <code>[Err](`Err`)([None])</code> is returned.
	///
	/// Iff `f` returns a [`Result::Err`], this error is returned wrapped in [`Some`].
	///
	/// # Panics
	///
	/// This method will panic if the instance has been poisoned *due to a panic* during a previous transformation attempt.
	///
	/// The method **may** panic (or deadlock) upon reentrance.
	pub fn get_or_create_or_poison<F, E>(&self, f: F) -> Result<&U, Option<E>>
	where
		F: FnOnce(T) -> Result<U, E>,
	{
		// In addition to being correct, this pattern is vouched for by Hans Boehm
		// (http://schd.ws/hosted_files/cppcon2016/74/HansWeakAtomics.pdf Page 27)
		#[allow(clippy::if_not_else)]
		if !self.initialized.load(Ordering::Acquire) {
			// We *may* not be initialized. We have to block to be certain.
			let _lock = self.lock.lock().unwrap();
			if !self.initialized.load(Ordering::Relaxed) {
				// Ok, we're definitely uninitialized.
				// Safe to fiddle with the UnsafeCell now, because we're locked,
				// and there can't be any outstanding references.
				//
				// However, since this function can return early without poisoning `self.lock`,
				// `self.value` is first overwritten with `None` to mark the instance as poisoned-by-error.
				let value = unsafe { &mut *self.value.get() };
				let this = match value.take() {
					None => return Err(None), // Poisoned by previous error.
					Some(ThisOrThat::This(t)) => t,
					Some(ThisOrThat::That(_)) => panic!(), // Can't already be initialized!
				};
				*value = Some(ThisOrThat::That(f(this)?));
				self.initialized.store(true, Ordering::Release);
			} else {
				// We raced, and someone else initialized us. We can fall
				// through now.
			}
		}

		// We're initialized, our value is immutable, no synchronization needed.
		Ok(self.extract().unwrap())
	}

	/// Get a reference to the transformed value, returning `Some(&U)` if the
	/// `LazyTransform<T, U>` has been transformed or `None` if it has not.  It
	/// is guaranteed that if a reference is returned it is to the transformed
	/// value inside the the `LazyTransform<T>`.
	pub fn get(&self) -> Option<&U> {
		if self.initialized.load(Ordering::Acquire) {
			// We're initialized, our value is immutable, no synchronization needed.
			self.extract()
		} else {
			None
		}
	}
}

// As `T` is only ever accessed when locked, it's enough if it's `Send` for `Self` to be `Sync`.
unsafe impl<T, U> Sync for LazyTransform<T, U>
where
	T: Send,
	U: Send + Sync,
{
}

impl<T, U> Clone for LazyTransform<T, U>
where
	T: Clone,
	U: Clone,
{
	fn clone(&self) -> Self {
		// Overall, this method is very similar to `get_or_create` and uses the same
		// soundness reasoning.

		if self.initialized.load(Ordering::Acquire) {
			Self {
				initialized: true.into(),
				lock: Mutex::default(),
				value: UnsafeCell::new(unsafe {
					// SAFETY:
					// Everything is initialized and immutable here, so lockless cloning is safe.
					(&*self.value.get()).clone()
				}),
			}
		} else {
			// We *may* not be initialized. We have to block here before accessing `value`,
			// which also synchronises the `initialized` load.
			let _lock = self.lock.lock().unwrap();
			Self {
				initialized: self.initialized.load(Ordering::Relaxed).into(),
				lock: Mutex::default(),
				value: UnsafeCell::new(unsafe {
					// SAFETY:
					// Exclusive access while `_lock` is held.
					(&*self.value.get()).clone()
				}),
			}
		}
	}

	fn clone_from(&mut self, source: &Self) {
		// Overall, this method is very similar to `get_or_create` and uses the same
		// soundness reasoning. It's implemented explicitly here to avoid a `Mutex` drop/new.

		if self.initialized.load(Ordering::Acquire) {
			unsafe {
				// SAFETY:
				// Everything is initialized and immutable here, so lockless cloning is safe.
				// It's still important to store `initialized` with correct ordering, though.
				*self.value.get() = (&*source.value.get()).clone();
				self.initialized.store(true, Ordering::Release);
			}
		} else {
			// `source` *may* not be initialized. We have to block here before accessing `value`,
			// which also synchronises the `initialized` load (and incidentally also the `initialized`
			// store due to the exclusive reference to `self`, so that can be `Relaxed` here too).
			let _lock = source.lock.lock().unwrap();
			unsafe {
				// SAFETY:
				// Exclusive access to `source` while `_lock` is held.
				*self.value.get() = (&*source.value.get()).clone();
				self.initialized.store(
					source.initialized.load(Ordering::Relaxed),
					Ordering::Relaxed,
				);
			}
		}
	}
}

impl<T, U> Default for LazyTransform<T, U>
where
	T: Default,
{
	fn default() -> Self {
		LazyTransform::new(T::default())
	}
}

/// `Lazy<T>` is a lazily initialized synchronized holder type.  You can think
/// of it as a `LazyTransform` where the initial type doesn't exist.
#[derive(Clone)]
pub struct Lazy<T> {
	inner: LazyTransform<(), T>,
}

impl<T> Lazy<T> {
	/// Construct a new, uninitialized `Lazy<T>`.
	#[must_use]
	pub fn new() -> Lazy<T> {
		Self::default()
	}

	/// Unwrap the contained value, returning `Some` if the `Lazy<T>` has been initialized
	/// or `None` if it has not.
	pub fn into_inner(self) -> Option<T> {
		self.inner.into_inner().ok()
	}
}

impl<T> Lazy<T> {
	/// Get a reference to the contained value, invoking `f` to create it
	/// if the `Lazy<T>` is uninitialized.  It is guaranteed that if multiple
	/// calls to `get_or_create` race, only one will invoke its closure, and
	/// every call will receive a reference to the newly created value.
	///
	/// The value stored in the `Lazy<T>` is immutable after the closure returns
	/// it, so think carefully about what you want to put inside!
	pub fn get_or_create<F>(&self, f: F) -> &T
	where
		F: FnOnce() -> T,
	{
		self.inner.get_or_create(|_| f())
	}

	/// Tries to get a reference to the contained value, invoking `f` to create it
	/// if the `Lazy<T>` is uninitialized.  It is guaranteed that if multiple
	/// calls to `get_or_create` race, only one will **successfully** invoke its
	/// closure, and every call will receive a reference to the newly created value.
	///
	/// The value stored in the `Lazy<T>` is immutable after the closure succeeds
	/// and returns it, so think carefully about what you want to put inside!
	///
	/// # Errors
	///
	/// Iff `f` returns a [`Result::Err`], this error is returned verbatim.
	pub fn try_get_or_create<F, E>(&self, f: F) -> Result<&T, E>
	where
		F: FnOnce() -> Result<T, E>,
	{
		self.inner.try_get_or_create(|_| f())
	}

	/// Get a reference to the contained value, returning `Some(ref)` if the
	/// `Lazy<T>` has been initialized or `None` if it has not.  It is
	/// guaranteed that if a reference is returned it is to the value inside
	/// the `Lazy<T>`.
	pub fn get(&self) -> Option<&T> {
		self.inner.get()
	}
}

// `#[derive(Default)]` automatically adds `T: Default` trait bound, but that
// is too restrictive, because `Lazy<T>` always has a default value for any `T`.
impl<T> Default for Lazy<T> {
	fn default() -> Self {
		Lazy {
			inner: LazyTransform::new(()),
		}
	}
}

impl<T> fmt::Debug for Lazy<T>
where
	T: fmt::Debug,
{
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		if let Some(v) = self.get() {
			f.write_fmt(format_args!("Lazy({:?})", v))
		} else {
			f.write_str("Lazy(<uninitialized>)")
		}
	}
}

#[cfg(test)]
extern crate scoped_pool;

#[cfg(test)]
mod tests {

	use super::{Lazy, LazyTransform};
	use scoped_pool::Pool;
	use std::{
		sync::atomic::{AtomicUsize, Ordering},
		thread, time,
	};

	#[test]
	fn test_lazy() {
		let lazy_value: Lazy<u8> = Lazy::new();

		assert_eq!(lazy_value.get(), None);

		let n = AtomicUsize::new(0);

		let pool = Pool::new(100);
		pool.scoped(|scope| {
			for _ in 0..100 {
				let lazy_ref = &lazy_value;
				let n_ref = &n;
				scope.execute(move || {
					let ten_millis = time::Duration::from_millis(10);
					thread::sleep(ten_millis);

					let value = *lazy_ref.get_or_create(|| {
						// Make everybody else wait on me, because I'm a jerk.
						thread::sleep(ten_millis);

						// Make this relaxed so it doesn't interfere with
						// Lazy internals at all.
						n_ref.fetch_add(1, Ordering::Relaxed);

						42
					});
					assert_eq!(value, 42);

					let value = lazy_ref.get();
					assert_eq!(value, Some(&42));
				});
			}
		});

		assert_eq!(n.load(Ordering::SeqCst), 1);
	}

	#[test]
	fn test_lazy_fallible() {
		let lazy_value: Lazy<u8> = Lazy::new();

		lazy_value.try_get_or_create(|| Err(())).unwrap_err();
		assert_eq!(lazy_value.get(), None);

		let n = AtomicUsize::new(0);

		let pool = Pool::new(100);
		pool.scoped(|scope| {
			for _ in 0..100 {
				let lazy_ref = &lazy_value;
				let n_ref = &n;
				scope.execute(move || {
					let ten_millis = time::Duration::from_millis(10);
					thread::sleep(ten_millis);

					let value = *lazy_ref
						.try_get_or_create(|| {
							// Make everybody else wait on me, because I'm a jerk.
							thread::sleep(ten_millis);

							// Make this relaxed so it doesn't interfere with
							// Lazy internals at all.
							n_ref.fetch_add(1, Ordering::Relaxed);

							Result::<_, ()>::Ok(42)
						})
						.unwrap();
					assert_eq!(value, 42);

					let value = lazy_ref.get();
					assert_eq!(value, Some(&42));
				});
			}
		});

		assert_eq!(n.load(Ordering::SeqCst), 1);
	}

	#[test]
	fn test_lazy_transform() {
		let lazy_value: LazyTransform<u8, u8> = LazyTransform::new(21);

		assert_eq!(lazy_value.get(), None);

		let n = AtomicUsize::new(0);

		let pool = Pool::new(100);
		pool.scoped(|scope| {
			for _ in 0..100 {
				let lazy_ref = &lazy_value;
				let n_ref = &n;
				scope.execute(move || {
					let ten_millis = time::Duration::from_millis(10);
					thread::sleep(ten_millis);

					let value = *lazy_ref.get_or_create(|v| {
						// Make everybody else wait on me, because I'm a jerk.
						thread::sleep(ten_millis);

						// Make this relaxed so it doesn't interfere with
						// Lazy internals at all.
						n_ref.fetch_add(1, Ordering::Relaxed);

						v * 2
					});
					assert_eq!(value, 42);

					let value = lazy_ref.get();
					assert_eq!(value, Some(&42));
				});
			}
		});

		assert_eq!(n.load(Ordering::SeqCst), 1);
	}

	#[test]
	fn test_lazy_transform_fallible() {
		let lazy_value: LazyTransform<u8, u8> = LazyTransform::new(21);

		lazy_value.try_get_or_create(|_| Err(())).unwrap_err();
		assert_eq!(lazy_value.get(), None);

		let n = AtomicUsize::new(0);

		let pool = Pool::new(100);
		pool.scoped(|scope| {
			for _ in 0..100 {
				let lazy_ref = &lazy_value;
				let n_ref = &n;
				scope.execute(move || {
					let ten_millis = time::Duration::from_millis(10);
					thread::sleep(ten_millis);

					let value = *lazy_ref
						.try_get_or_create(|v| {
							// Make everybody else wait on me, because I'm a jerk.
							thread::sleep(ten_millis);

							// Make this relaxed so it doesn't interfere with
							// Lazy internals at all.
							n_ref.fetch_add(1, Ordering::Relaxed);

							Result::<_, ()>::Ok(v * 2)
						})
						.unwrap();
					assert_eq!(value, 42);

					let value = lazy_ref.get();
					assert_eq!(value, Some(&42));
				});
			}
		});

		assert_eq!(n.load(Ordering::SeqCst), 1);
	}
}
