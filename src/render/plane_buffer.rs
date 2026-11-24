#![allow(unsafe_code)]
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;
pub(crate) struct PlaneBuffer<T> {
    ptr: *mut T,
    len: usize,
    cap: usize,
}
unsafe impl<T: Send> Send for PlaneBuffer<T> {}
unsafe impl<T: Send + Sync> Sync for PlaneBuffer<T> {}
impl<T> PlaneBuffer<T> {
    pub(crate) fn new(values: Vec<T>) -> Arc<PlaneBuffer<T>> {
        let mut values = std::mem::ManuallyDrop::new(values);
        Arc::new(PlaneBuffer {
            ptr: values.as_mut_ptr(),
            len: values.len(),
            cap: values.capacity(),
        })
    }
    pub(crate) fn len(&self) -> usize {
        self.len
    }
    pub(crate) unsafe fn range(&self, at: usize, count: usize) -> &[T] {
        assert!(
            at <= self.len && count <= self.len - at,
            "plane outside the buffer"
        );
        unsafe { std::slice::from_raw_parts(self.ptr.add(at), count) }
    }
    #[allow(clippy::mut_from_ref)]
    pub(crate) unsafe fn range_mut(&self, at: usize, count: usize) -> &mut [T] {
        assert!(
            at <= self.len && count <= self.len - at,
            "plane outside the buffer"
        );
        unsafe { std::slice::from_raw_parts_mut(self.ptr.add(at), count) }
    }
}
impl<T> Drop for PlaneBuffer<T> {
    fn drop(&mut self) {
        unsafe { drop(Vec::from_raw_parts(self.ptr, self.len, self.cap)) }
    }
}
#[derive(Clone)]
pub(crate) struct Samples<T>(Arc<PlaneBuffer<T>>);
impl<T> Samples<T> {
    pub(crate) fn finished(buffer: Arc<PlaneBuffer<T>>) -> Samples<T> {
        Samples(buffer)
    }
}
impl<T> From<Vec<T>> for Samples<T> {
    fn from(values: Vec<T>) -> Samples<T> {
        Samples(PlaneBuffer::new(values))
    }
}
impl<T> Deref for Samples<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        unsafe { self.0.range(0, self.0.len()) }
    }
}
impl<T> fmt::Debug for Samples<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Samples")
            .field("len", &self.0.len())
            .finish()
    }
}
pub(crate) trait PlainSample: Copy + Send + Sync + 'static + sealed::Sealed {}
mod sealed {
    pub(crate) trait Sealed {}
    impl Sealed for i16 {}
    impl Sealed for u16 {}
    impl Sealed for f32 {}
}
impl PlainSample for i16 {}
impl PlainSample for u16 {}
impl PlainSample for f32 {}
pub(crate) fn le_bytes<T: PlainSample>(values: &[T]) -> Option<&[u8]> {
    if !cfg!(target_endian = "little") {
        return None;
    }
    Some(unsafe {
        std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), std::mem::size_of_val(values))
    })
}
#[cfg(test)]
mod tests;
