// src/handles.rs

//! Kernel object handles — typed wrappers around raw IDs.
//!
//! No raw IDs are exposed between subsystems. Each handle type
//! is a newtype wrapper that can only be created by the owning subsystem.

use core::marker::PhantomData;

/// Marker trait for kernel object handles.
pub trait Handle {}

/// Generic typed handle. `T` is the marker type (e.g., `TaskHandle`,
/// `FileHandle`), and `ID` is the underlying integer type.
#[repr(transparent)]
pub struct GenericHandle<T: Handle, ID: Copy> {
    id: ID,
    _marker: PhantomData<T>,
}

impl<T: Handle, ID: Copy> GenericHandle<T, ID> {
    pub fn new(id: ID) -> Self {
        Self { id, _marker: PhantomData }
    }
    pub fn id(&self) -> ID { self.id }
}

impl<T: Handle, ID: Copy> Copy for GenericHandle<T, ID> {}
impl<T: Handle, ID: Copy> Clone for GenericHandle<T, ID> {
    fn clone(&self) -> Self { *self }
}

// ---- Concrete handle types ----

pub struct TaskMarker;
pub struct ProcessMarker;
pub struct FileHandleTag;
pub struct DeviceHandleTag;

pub type TaskHandle = GenericHandle<TaskMarker, usize>;
pub type ProcessHandle = GenericHandle<ProcessMarker, usize>;
pub type FileHandle = GenericHandle<FileHandleTag, usize>;
pub type DeviceHandle = GenericHandle<DeviceHandleTag, usize>;

impl Handle for TaskMarker {}
impl Handle for ProcessMarker {}
impl Handle for FileHandleTag {}
impl Handle for DeviceHandleTag {}
