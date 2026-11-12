//! The first explicit primitive-composition subset of a Forma control template.
//! The host supplies the bounds; every painted primitive comes from this file.

use crate::markup::ButtonSpec;
use memory_budget::{CacheLimits, WeightedLru};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

mod cache;
mod lexer;
mod model;
mod parser;

pub use cache::*;
use lexer::*;
pub use model::*;
pub use parser::*;

#[cfg(test)]
mod tests;
