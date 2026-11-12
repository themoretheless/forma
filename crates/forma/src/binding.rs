//! Typed, UI-thread bindings used by generated forms. Model notifications only
//! mark bindings dirty; `Form::update` / `Form::sync` apply them before painting.

use crate::Runtime;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

mod context;
mod control;
mod field;
mod form;
mod input;
mod property;
mod scene;
mod update;

pub use context::*;
pub use control::*;
use field::*;
pub use form::*;
use input::*;
pub use property::*;
use scene::*;
use update::*;

mod model_value;

pub use model_value::{ModelNumber, ModelValue, decode};

#[cfg(test)]
mod tests;
