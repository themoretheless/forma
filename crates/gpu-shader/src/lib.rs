pub mod back;
pub mod cad_kernels;
pub mod compiler;
pub mod front;
pub mod proc;
pub mod valid;

pub use cad_kernels::*;
pub use compiler::*;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Index;

pub struct Handle<T> {
    index: usize,
    _marker: std::marker::PhantomData<fn() -> T>,
}

impl<T> Clone for Handle<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> PartialEq for Handle<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl<T> Eq for Handle<T> {}

impl<T> PartialOrd for Handle<T> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Handle<T> {
    #[inline]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.index.cmp(&other.index)
    }
}

impl<T> std::hash::Hash for Handle<T> {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handle({})", self.index)
    }
}

impl<T> Handle<T> {
    #[inline]
    pub const fn new(index: usize) -> Self {
        Self {
            index,
            _marker: std::marker::PhantomData,
        }
    }

    #[inline]
    pub const fn index(self) -> usize {
        self.index
    }
}

#[derive(Debug, Clone)]
pub struct Arena<T> {
    items: Vec<T>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

impl<T> Arena<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, item: T) -> Handle<T> {
        let index = self.items.len();
        self.items.push(item);
        Handle::new(index)
    }

    pub fn iter(&self) -> impl Iterator<Item = (Handle<T>, &T)> {
        self.items
            .iter()
            .enumerate()
            .map(|(i, v)| (Handle::new(i), v))
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        self.items.get(handle.index)
    }
}

impl<T> Index<Handle<T>> for Arena<T> {
    type Output = T;

    #[inline]
    fn index(&self, handle: Handle<T>) -> &Self::Output {
        &self.items[handle.index]
    }
}

pub type UniqueArena<T> = Arena<T>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    Compute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageAccess {
    Read,
    Write,
    ReadWrite,
}

impl StorageAccess {
    pub const LOAD: Self = Self::Read;
    pub const STORE: Self = Self::Write;

    #[inline]
    pub fn contains(self, other: Self) -> bool {
        match (self, other) {
            (Self::ReadWrite, _) => true,
            (Self::Read, Self::Read) => true,
            (Self::Write, Self::Write) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AddressSpace {
    Function,
    Private,
    WorkGroup,
    Uniform,
    Storage { access: StorageAccess },
    Handle,
    Immediate,
    PushConstant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceBinding {
    pub group: u32,
    pub binding: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScalarKind {
    Sint,
    Uint,
    Float,
    Bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Scalar {
    pub kind: ScalarKind,
    pub width: u8,
}

impl Scalar {
    pub const I32: Self = Self {
        kind: ScalarKind::Sint,
        width: 4,
    };
    pub const U32: Self = Self {
        kind: ScalarKind::Uint,
        width: 4,
    };
    pub const F32: Self = Self {
        kind: ScalarKind::Float,
        width: 4,
    };
    pub const F16: Self = Self {
        kind: ScalarKind::Float,
        width: 2,
    };
    pub const BOOL: Self = Self {
        kind: ScalarKind::Bool,
        width: 1,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VectorSize {
    Bi = 2,
    Tri = 3,
    Quad = 4,
}

impl VectorSize {
    #[inline]
    pub const fn as_u32(self) -> u32 {
        match self {
            Self::Bi => 2,
            Self::Tri => 3,
            Self::Quad => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArraySize {
    Constant( std::num::NonZeroU32 ),
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructMember {
    pub name: Option<String>,
    pub ty: Handle<Type>,
    pub binding: Option<Binding>,
    pub offset: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltIn {
    Position,
    VertexIndex,
    InstanceIndex,
    FrontFacing,
    FragDepth,
    GlobalInvocationId,
    LocalInvocationId,
    LocalInvocationIndex,
    WorkGroupId,
    NumWorkGroups,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Interpolation {
    Perspective,
    Linear,
    Flat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sampling {
    Center,
    Centroid,
    Sample,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Binding {
    BuiltIn(BuiltIn),
    Location {
        location: u32,
        interpolation: Option<Interpolation>,
        sampling: Option<Sampling>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageDimension {
    D1,
    D2,
    D3,
    Cube,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageClass {
    Sampled { kind: ScalarKind, multi: bool },
    Depth { multi: bool },
    Storage { format: StorageFormat, access: StorageAccess },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageFormat {
    Rgba8Unorm,
    Rgba32Float,
    R32Float,
    R32Uint,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeInner {
    Scalar(Scalar),
    Vector {
        size: VectorSize,
        scalar: Scalar,
    },
    Matrix {
        columns: VectorSize,
        rows: VectorSize,
        scalar: Scalar,
    },
    Atomic(Scalar),
    Pointer {
        base: Handle<Type>,
        space: AddressSpace,
    },
    Array {
        base: Handle<Type>,
        size: ArraySize,
        stride: u32,
    },
    Struct {
        members: Vec<StructMember>,
        span: u32,
    },
    Image {
        dim: ImageDimension,
        arrayed: bool,
        class: ImageClass,
    },
    Sampler {
        comparison: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Type {
    pub name: Option<String>,
    pub inner: TypeInner,
}

#[derive(Debug, Clone)]
pub struct GlobalVariable {
    pub name: Option<String>,
    pub space: AddressSpace,
    pub binding: Option<ResourceBinding>,
    pub ty: Handle<Type>,
    pub init: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Constant {
    pub name: Option<String>,
    pub ty: Option<Handle<Type>>,
    pub expr_src: String,
    pub u32_value: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct FunctionArgument {
    pub name: Option<String>,
    pub ty: Handle<Type>,
    pub binding: Option<Binding>,
    pub raw_ty_src: String,
}

#[derive(Debug, Clone)]
pub struct FunctionResult {
    pub ty: Handle<Type>,
    pub binding: Option<Binding>,
    pub raw_ty_src: String,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: Option<String>,
    pub arguments: Vec<FunctionArgument>,
    pub result: Option<FunctionResult>,
    pub body_src: String,
    pub used_globals: BTreeSet<Handle<GlobalVariable>>,
    pub called_functions: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct EntryPoint {
    pub name: String,
    pub stage: ShaderStage,
    pub workgroup_size: [u32; 3],
    pub workgroup_size_overrides: Option<[Option<String>; 3]>,
    pub workgroup_size_exprs: [String; 3],
    pub function: Function,
}

#[derive(Debug, Clone, Default)]
pub struct Module {
    pub types: UniqueArena<Type>,
    pub constants: Arena<Constant>,
    pub global_variables: Arena<GlobalVariable>,
    pub functions: Arena<Function>,
    pub entry_points: Vec<EntryPoint>,
    pub aliases: BTreeMap<String, String>,
    pub struct_order: Vec<Handle<Type>>,
    pub raw_source: String,
}

impl Module {
    pub fn to_ctx(&self) -> proc::GlobalCtx<'_> {
        proc::GlobalCtx {
            types: &self.types,
        }
    }
}

impl fmt::Display for ShaderStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vertex => write!(f, "vertex"),
            Self::Fragment => write!(f, "fragment"),
            Self::Compute => write!(f, "compute"),
        }
    }
}
