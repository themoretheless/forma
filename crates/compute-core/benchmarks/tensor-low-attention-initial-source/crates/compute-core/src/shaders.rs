//! Generic building-block kernels, embedded at compile time.
//!
//! `WG` anchor convention: tunable sources declare
//! `const WG: u32 = 256;` and `@workgroup_size(WG)`; the runtime may
//! substitute a tuned power-of-two before compilation.

pub const SCALE_ADD_WGSL: &str = include_str!("../shaders/scale_add.wgsl");
pub const ZIP_MUL_WGSL: &str = include_str!("../shaders/zip_mul.wgsl");
pub const BLOCK_SUM_WGSL: &str = include_str!("../shaders/block_sum.wgsl");
/// vec4 variants: 16 bytes streamed per thread instead of 4.
pub const SCALE_ADD4_WGSL: &str = include_str!("../shaders/scale_add4.wgsl");
pub const ZIP_MUL4_WGSL: &str = include_str!("../shaders/zip_mul4.wgsl");

pub const UNARY_WGSL: &str = include_str!("../shaders/unary.wgsl");
pub const AFFINE_WGSL: &str = include_str!("../shaders/affine.wgsl");
pub const BINARY_WGSL: &str = include_str!("../shaders/binary.wgsl");
pub const COMPARE_WGSL: &str = include_str!("../shaders/compare.wgsl");

pub const SCAN_BLOCKS_WGSL: &str = include_str!("../shaders/scan_blocks.wgsl");
/// Four consecutive elements per lane; block capacity is 4 * workgroup size.
pub const SCAN_BLOCKS4_WGSL: &str = include_str!("../shaders/scan_blocks4.wgsl");
pub const SCAN_ADD_WGSL: &str = include_str!("../shaders/scan_add.wgsl");
pub const COMPACT_SCATTER_WGSL: &str = include_str!("../shaders/compact_scatter.wgsl");
pub const COMPACT_SCATTER_LOCAL_WGSL: &str = include_str!("../shaders/compact_scatter_local.wgsl");
pub const COMPACT_FINISH_WGSL: &str = include_str!("../shaders/compact_finish.wgsl");

/// Fixed 32x32 tile geometry; workgroup tuning does not apply.
pub const MATMUL_WGSL: &str = include_str!("../shaders/matmul.wgsl");

/// Requires all matrix dimensions to be positive multiples of 32.
pub const MATMUL_ALIGNED_WGSL: &str = include_str!("../shaders/matmul_aligned.wgsl");
pub const MATMUL_DIRECT_WGSL: &str = include_str!("../shaders/matmul_direct.wgsl");
pub const MATMUL_DOT_WGSL: &str = include_str!("../shaders/matmul_dot.wgsl");
pub const TENSOR_ELEMENTWISE_WGSL: &str = include_str!("../shaders/tensor_elementwise.wgsl");
pub const TENSOR_SUM_WGSL: &str = include_str!("../shaders/tensor_sum.wgsl");
pub const TENSOR_REDUCE_WGSL: &str = include_str!("../shaders/tensor_reduce.wgsl");
pub const TENSOR_MATMUL_WGSL: &str = include_str!("../shaders/tensor_matmul.wgsl");

pub const TENSOR_COPY_WGSL: &str = include_str!("../shaders/tensor_copy.wgsl");
pub const TENSOR_COMPARE_WGSL: &str = include_str!("../shaders/tensor_compare.wgsl");
pub const TENSOR_SELECT_WGSL: &str = include_str!("../shaders/tensor_select.wgsl");
pub const TENSOR_SCAN_BLOCKS_WGSL: &str = include_str!("../shaders/tensor_scan_blocks.wgsl");
pub const TENSOR_SCAN_ADD_WGSL: &str = include_str!("../shaders/tensor_scan_add.wgsl");
pub const TENSOR_GATHER_WGSL: &str = include_str!("../shaders/tensor_gather.wgsl");
pub const TENSOR_INDEX_COUNT_WGSL: &str = include_str!("../shaders/tensor_index_count.wgsl");
pub const TENSOR_COUNT_RESET_WGSL: &str = include_str!("../shaders/tensor_count_reset.wgsl");
pub const TENSOR_COMPACT_WGSL: &str = include_str!("../shaders/tensor_compact.wgsl");
pub const TENSOR_SCATTER_WGSL: &str = include_str!("../shaders/tensor_scatter.wgsl");
pub const TENSOR_SCATTER_RESET_WGSL: &str = include_str!("../shaders/tensor_scatter_reset.wgsl");
pub const TENSOR_SCATTER_OWNERS_WGSL: &str = include_str!("../shaders/tensor_scatter_owners.wgsl");
pub const TENSOR_LOW_PACK_WGSL: &str = concat!(
    include_str!("../shaders/low_codec.wgsl"),
    "\n",
    include_str!("../shaders/tensor_low_pack.wgsl")
);
pub const TENSOR_LOW_UNPACK_WGSL: &str = concat!(
    include_str!("../shaders/low_codec.wgsl"),
    "\n",
    include_str!("../shaders/tensor_low_unpack.wgsl")
);
pub const TENSOR_LOW_MATMUL_WGSL: &str = concat!(
    include_str!("../shaders/low_codec.wgsl"),
    "\n",
    include_str!("../shaders/tensor_low_matmul.wgsl")
);
pub const TENSOR_LOW_ARITHMETIC_WGSL: &str = concat!(
    include_str!("../shaders/low_codec.wgsl"),
    "\n",
    include_str!("../shaders/float_bits_order.wgsl"),
    "\n",
    include_str!("../shaders/tensor_low_arithmetic.wgsl")
);

pub const TENSOR_STATS_REDUCE_WGSL: &str = concat!(
    include_str!("../shaders/float_power2.wgsl"),
    "\n",
    include_str!("../shaders/tensor_stats_common.wgsl"),
    "\n",
    include_str!("../shaders/tensor_stats_reduce.wgsl")
);
pub const TENSOR_STATS_FINISH_WGSL: &str = concat!(
    include_str!("../shaders/float_power2.wgsl"),
    "\n",
    include_str!("../shaders/tensor_stats_common.wgsl"),
    "\n",
    include_str!("../shaders/tensor_stats_finish.wgsl")
);
pub const TENSOR_STATS_OUTPUT_WGSL: &str = concat!(
    include_str!("../shaders/float_power2.wgsl"),
    "\n",
    include_str!("../shaders/tensor_stats_common.wgsl"),
    "\n",
    include_str!("../shaders/tensor_stats_output.wgsl")
);
pub const TENSOR_STATS_SMALL_WGSL: &str = concat!(
    include_str!("../shaders/float_power2.wgsl"),
    "\n",
    include_str!("../shaders/tensor_stats_common.wgsl"),
    "\n",
    include_str!("../shaders/tensor_stats_small.wgsl")
);
pub const TENSOR_ATTENTION_WGSL: &str = concat!(
    include_str!("../shaders/float_power2.wgsl"),
    "\n",
    include_str!("../shaders/tensor_attention_common.wgsl"),
    "\n",
    include_str!("../shaders/tensor_attention.wgsl")
);
pub const TENSOR_ATTENTION_MERGE_WGSL: &str = concat!(
    include_str!("../shaders/float_power2.wgsl"),
    "\n",
    include_str!("../shaders/tensor_attention_common.wgsl"),
    "\n",
    include_str!("../shaders/tensor_attention_merge.wgsl")
);

/// Every shipped kernel (name, source), for validation tests.
pub const ALL: [(&str, &str); 45] = [
    ("tensor_low_arithmetic", TENSOR_LOW_ARITHMETIC_WGSL),
    ("tensor_attention_merge", TENSOR_ATTENTION_MERGE_WGSL),
    ("tensor_attention", TENSOR_ATTENTION_WGSL),
    ("tensor_stats_small", TENSOR_STATS_SMALL_WGSL),
    ("tensor_stats_reduce", TENSOR_STATS_REDUCE_WGSL),
    ("tensor_stats_finish", TENSOR_STATS_FINISH_WGSL),
    ("tensor_stats_output", TENSOR_STATS_OUTPUT_WGSL),
    ("tensor_low_pack", TENSOR_LOW_PACK_WGSL),
    ("tensor_low_unpack", TENSOR_LOW_UNPACK_WGSL),
    ("tensor_low_matmul", TENSOR_LOW_MATMUL_WGSL),
    ("tensor_scatter", TENSOR_SCATTER_WGSL),
    ("tensor_scatter_reset", TENSOR_SCATTER_RESET_WGSL),
    ("tensor_scatter_owners", TENSOR_SCATTER_OWNERS_WGSL),
    ("tensor_reduce", TENSOR_REDUCE_WGSL),
    ("tensor_copy", TENSOR_COPY_WGSL),
    ("tensor_compare", TENSOR_COMPARE_WGSL),
    ("tensor_select", TENSOR_SELECT_WGSL),
    ("tensor_scan_blocks", TENSOR_SCAN_BLOCKS_WGSL),
    ("tensor_scan_add", TENSOR_SCAN_ADD_WGSL),
    ("tensor_gather", TENSOR_GATHER_WGSL),
    ("tensor_index_count", TENSOR_INDEX_COUNT_WGSL),
    ("tensor_count_reset", TENSOR_COUNT_RESET_WGSL),
    ("tensor_compact", TENSOR_COMPACT_WGSL),
    ("tensor_elementwise", TENSOR_ELEMENTWISE_WGSL),
    ("tensor_sum", TENSOR_SUM_WGSL),
    ("tensor_matmul", TENSOR_MATMUL_WGSL),
    ("scan_blocks", SCAN_BLOCKS_WGSL),
    ("scan_blocks4", SCAN_BLOCKS4_WGSL),
    ("scan_add", SCAN_ADD_WGSL),
    ("compact_scatter", COMPACT_SCATTER_WGSL),
    ("compact_scatter_local", COMPACT_SCATTER_LOCAL_WGSL),
    ("compact_finish", COMPACT_FINISH_WGSL),
    ("unary", UNARY_WGSL),
    ("affine", AFFINE_WGSL),
    ("binary", BINARY_WGSL),
    ("compare", COMPARE_WGSL),
    ("matmul", MATMUL_WGSL),
    ("matmul_aligned", MATMUL_ALIGNED_WGSL),
    ("matmul_direct", MATMUL_DIRECT_WGSL),
    ("matmul_dot", MATMUL_DOT_WGSL),
    ("scale_add", SCALE_ADD_WGSL),
    ("zip_mul", ZIP_MUL_WGSL),
    ("block_sum", BLOCK_SUM_WGSL),
    ("scale_add4", SCALE_ADD4_WGSL),
    ("zip_mul4", ZIP_MUL4_WGSL),
];
