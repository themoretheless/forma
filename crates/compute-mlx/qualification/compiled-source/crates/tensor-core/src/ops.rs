/// Elementwise f32 arithmetic. Backends preserve these discriminants; portable
/// results are intended for finite inputs with nonzero divisors. Reduction
/// order differs from a sequential CPU sum, so compare with a suitable tolerance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Min,
    Max,
}

/// Elementwise f32 math. Callers must respect each function's domain:
/// sqrt requires nonnegative inputs, log positive inputs, reciprocal nonzero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum UnaryOp {
    Negate,
    Abs,
    Square,
    Sqrt,
    Reciprocal,
    Exp,
    Log,
    Sin,
    Cos,
}

/// Elementwise comparisons yielding a u32 zero/one mask. The backend trait
/// defines the input dtype and numerical domain: the f32 contract covers finite
/// inputs, u32 compares exact integers, and the low-storage contract additionally
/// defines IEEE NaN, infinity and signed-zero behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum CompareOp {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}
