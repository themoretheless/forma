use crate::{ArraySize, Handle, Scalar, Type, TypeInner, UniqueArena, VectorSize};
use std::ops::Index;

#[derive(Debug, Clone, Copy)]
pub struct GlobalCtx<'a> {
    pub types: &'a UniqueArena<Type>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TypeLayout {
    pub size: u32,
    pub alignment: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Layouter {
    layouts: Vec<TypeLayout>,
}

impl Layouter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, ctx: GlobalCtx<'_>) -> Result<(), String> {
        self.layouts.clear();
        self.layouts.reserve(ctx.types.len());
        for (handle, _) in ctx.types.iter() {
            let layout = compute_type_layout(ctx.types, handle);
            self.layouts.push(layout);
        }
        Ok(())
    }
}

impl Index<Handle<Type>> for Layouter {
    type Output = TypeLayout;

    #[inline]
    fn index(&self, handle: Handle<Type>) -> &Self::Output {
        &self.layouts[handle.index()]
    }
}

#[inline]
pub const fn align_up(value: u32, align: u32) -> u32 {
    if align <= 1 {
        value
    } else {
        (value + align - 1) & !(align - 1)
    }
}

pub fn scalar_layout(scalar: Scalar) -> TypeLayout {
    let w = (scalar.width as u32).max(4);
    TypeLayout {
        size: w,
        alignment: w,
    }
}

pub fn vector_layout(size: VectorSize, scalar: Scalar) -> TypeLayout {
    let elem = (scalar.width as u32).max(2);
    match size {
        VectorSize::Bi => TypeLayout {
            size: elem * 2,
            alignment: elem * 2,
        },
        VectorSize::Tri => TypeLayout {
            size: elem * 3,
            alignment: elem * 4,
        },
        VectorSize::Quad => TypeLayout {
            size: elem * 4,
            alignment: elem * 4,
        },
    }
}

pub fn compute_type_layout(types: &UniqueArena<Type>, handle: Handle<Type>) -> TypeLayout {
    match &types[handle].inner {
        TypeInner::Scalar(s) => scalar_layout(*s),
        TypeInner::Vector { size, scalar } => vector_layout(*size, *scalar),
        TypeInner::Matrix {
            columns,
            rows,
            scalar,
        } => {
            let col_layout = vector_layout(*rows, *scalar);
            let col_stride = align_up(col_layout.size, col_layout.alignment);
            TypeLayout {
                size: col_stride * columns.as_u32(),
                alignment: col_layout.alignment,
            }
        }
        TypeInner::Atomic(s) => scalar_layout(*s),
        TypeInner::Pointer { .. } => TypeLayout {
            size: 8,
            alignment: 8,
        },
        TypeInner::Array { base, size, stride } => {
            let base_layout = compute_type_layout(types, *base);
            let elem_stride = if *stride > 0 {
                *stride
            } else {
                align_up(base_layout.size, base_layout.alignment)
            };
            let count = match size {
                ArraySize::Constant(n) => n.get(),
                ArraySize::Dynamic => 1,
            };
            TypeLayout {
                size: elem_stride * count,
                alignment: base_layout.alignment,
            }
        }
        TypeInner::Struct { members, span } => {
            if *span > 0 && !members.is_empty() {
                let mut max_align = 1u32;
                for m in members {
                    let ml = compute_type_layout(types, m.ty);
                    max_align = max_align.max(ml.alignment);
                }
                return TypeLayout {
                    size: align_up(*span, max_align),
                    alignment: max_align,
                };
            }
            let mut offset = 0u32;
            let mut max_align = 1u32;
            for m in members {
                let ml = compute_type_layout(types, m.ty);
                max_align = max_align.max(ml.alignment);
                offset = align_up(offset, ml.alignment);
                offset += ml.size;
            }
            TypeLayout {
                size: align_up(offset, max_align),
                alignment: max_align,
            }
        }
        TypeInner::Image { .. } | TypeInner::Sampler { .. } => TypeLayout {
            size: 0,
            alignment: 1,
        },
    }
}
