use crate::{GlobalVariable, Handle, Module};
use std::collections::BTreeSet;
use std::fmt;
use std::ops::Index;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationFlags(u32);

impl ValidationFlags {
    pub const fn all() -> Self {
        Self(0xFFFF_FFFF)
    }

    pub const fn empty() -> Self {
        Self(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities(u32);

impl Capabilities {
    pub const fn all() -> Self {
        Self(0xFFFF_FFFF)
    }

    pub const fn empty() -> Self {
        Self(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalUse(u8);

impl GlobalUse {
    pub const EMPTY: Self = Self(0);
    pub const READ: Self = Self(1);
    pub const WRITE: Self = Self(2);
    pub const READ_WRITE: Self = Self(3);

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

#[derive(Debug, Clone)]
pub struct FunctionInfo {
    globals: Vec<GlobalUse>,
}

impl FunctionInfo {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.globals.iter().all(|g| g.is_empty())
    }
}

impl Index<Handle<GlobalVariable>> for FunctionInfo {
    type Output = GlobalUse;

    #[inline]
    fn index(&self, handle: Handle<GlobalVariable>) -> &Self::Output {
        self.globals.get(handle.index()).unwrap_or(&GlobalUse::EMPTY)
    }
}

#[derive(Debug, Clone)]
pub struct ModuleInfo {
    functions: Vec<FunctionInfo>,
    entry_points: Vec<FunctionInfo>,
}

impl ModuleInfo {
    #[inline]
    pub fn get_entry_point(&self, index: usize) -> &FunctionInfo {
        &self.entry_points[index]
    }

    #[inline]
    pub fn get_function(&self, index: usize) -> &FunctionInfo {
        &self.functions[index]
    }
}

#[derive(Debug, Clone)]
pub struct ValidationError {
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ValidationError {}

pub struct Validator {
    _flags: ValidationFlags,
    _capabilities: Capabilities,
}

impl Validator {
    pub fn new(flags: ValidationFlags, capabilities: Capabilities) -> Self {
        Self {
            _flags: flags,
            _capabilities: capabilities,
        }
    }

    pub fn validate(&mut self, module: &Module) -> Result<ModuleInfo, ValidationError> {
        let num_globals = module.global_variables.len();

        let mut functions = Vec::with_capacity(module.functions.len());
        for (_, func) in module.functions.iter() {
            let mut visited = BTreeSet::new();
            let used = collect_transitive_globals(module, func, &mut visited);
            let mut globals = vec![GlobalUse::EMPTY; num_globals];
            for g in used {
                if g.index() < globals.len() {
                    globals[g.index()] = GlobalUse::READ_WRITE;
                }
            }
            functions.push(FunctionInfo { globals });
        }

        let mut entry_points = Vec::with_capacity(module.entry_points.len());
        for ep in &module.entry_points {
            let mut visited = BTreeSet::new();
            let used = collect_transitive_globals(module, &ep.function, &mut visited);
            let mut globals = vec![GlobalUse::EMPTY; num_globals];
            for g in used {
                if g.index() < globals.len() {
                    globals[g.index()] = GlobalUse::READ_WRITE;
                }
            }
            entry_points.push(FunctionInfo { globals });
        }

        Ok(ModuleInfo {
            functions,
            entry_points,
        })
    }
}

fn collect_transitive_globals(
    module: &Module,
    func: &crate::Function,
    visited_funcs: &mut BTreeSet<String>,
) -> BTreeSet<Handle<GlobalVariable>> {
    let mut used = func.used_globals.clone();
    for callee_name in &func.called_functions {
        if visited_funcs.insert(callee_name.clone()) {
            for (_, f) in module.functions.iter() {
                if f.name.as_deref() == Some(callee_name.as_str()) {
                    let sub = collect_transitive_globals(module, f, visited_funcs);
                    used.extend(sub);
                }
            }
        }
    }
    used
}
