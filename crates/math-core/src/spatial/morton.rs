//! Morton codes.

use super::*;

/// Spreads the lowest 10 bits of `v` so there are 2 zero bits between each bit, producing a 30-bit
/// mask ready for 3D Morton (Z-order curve) interleaving.
#[inline]
pub const fn expand_bits_10_to_30(v: u32) -> u32 {
    let mut x = v & 0x0000_03FF;
    x = (x.wrapping_mul(0x0001_0001)) & 0xFF00_00FF;
    x = (x.wrapping_mul(0x0000_0101)) & 0x0F00_F00F;
    x = (x.wrapping_mul(0x0000_0011)) & 0xC30C_30C3;
    x = (x.wrapping_mul(0x0000_0005)) & 0x4924_9249;
    x
}

/// Compacts every third bit of a 30-bit Morton word back into a 10-bit integer in `0..=1023`.
#[inline]
const fn compact_bits_30_to_10(v: u32) -> u32 {
    let mut x = v & 0x4924_9249;
    x = (x ^ (x >> 2)) & 0xC30C_30C3;
    x = (x ^ (x >> 4)) & 0x0F00_F00F;
    x = (x ^ (x >> 8)) & 0xFF00_00FF;
    x = (x ^ (x >> 16)) & 0x0000_03FF;
    x
}

/// Encodes 10-bit integer coordinates `(x, y, z)` (clamped/masked to `0..=1023`) into a 30-bit
/// 3D Z-order Morton code (`x` at bit 0, `y` at bit 1, `z` at bit 2).
#[inline]
pub const fn morton3d_encode(x: u32, y: u32, z: u32) -> u32 {
    expand_bits_10_to_30(x) | (expand_bits_10_to_30(y) << 1) | (expand_bits_10_to_30(z) << 2)
}

/// Decodes a 30-bit 3D Z-order Morton code back into its 10-bit `(x, y, z)` integer coordinates.
#[inline]
pub const fn morton3d_decode(code: u32) -> (u32, u32, u32) {
    (
        compact_bits_30_to_10(code),
        compact_bits_30_to_10(code >> 1),
        compact_bits_30_to_10(code >> 2),
    )
}

/// Quantizes a normalized 3D point `p` in $[0, 1]^3$ onto a $1024^3$ lattice and returns its
/// 30-bit 3D Z-order Morton code for GPU/CPU Linear BVH (`LBVH`) construction.
#[inline]
pub fn morton3d_from_unit_point(p: V3) -> u32 {
    let quantize = |c: f64| -> u32 {
        if !c.is_finite() {
            0
        } else {
            (c.clamp(0.0, 1.0) * 1023.0).round().clamp(0.0, 1023.0) as u32
        }
    };
    morton3d_encode(quantize(p[0]), quantize(p[1]), quantize(p[2]))
}
