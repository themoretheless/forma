//! GPU Adapter, Device, Limits, Error Scope, Poll, and Descriptor types.

use crate::{Backends, Features, InstanceFlags, Label};
use std::future::Future;
use std::sync::{Arc, Mutex};

mod adapter;
mod device;
mod error_scope;
mod poll;
mod surface;

pub use adapter::*;
pub use device::*;
pub use error_scope::*;
pub use poll::*;
pub use surface::*;

#[cfg(test)]
mod scope_signal_tests;
