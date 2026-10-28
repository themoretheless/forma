// The common traversal kernels specialize storage and every scan intermediate
// to double. Masks, indices, elected owners and counts remain unsigned.
extern "C" __global__ void copy_f64(const double* input, double* output,
    const U64* layout, U64 count, unsigned rank, U64 offset) {
    copy_view_impl(input, output, layout, count, rank, offset);
}
COMPARE(double, compare_f64)
WHERE(double, where_f64)
SCAN(double, scan_f64)
ADD_SCAN(double, add_scan_f64)
GATHER(double, gather_f64)
COMPACT(double, compact_f64)

// Compare integer bits for retry termination. All arithmetic, including the
// multiply path, remains binary64 and does not depend on native atomicAdd.
__device__ void scatter_fold_f64(double* address, double update, unsigned op) {
    U64* bits = reinterpret_cast<U64*>(address);
    U64 old = atomicCAS(bits, 0ULL, 0ULL);
    U64 assumed;
    do {
        assumed = old;
        const double current = __longlong_as_double((long long)assumed);
        double next = current;
        switch (op) {
            case 1: next = current + update; break;
            case 2: next = current * update; break;
            case 3: next = reduce_combine(current, update, 2); break;
            case 4: next = reduce_combine(current, update, 3); break;
        }
        old = atomicCAS(bits, assumed, (U64)__double_as_longlong(next));
    } while (old != assumed);
}
template <> struct ScatterTypedWrite<double> {
    __device__ void operator()(double* address, double update, unsigned op) const {
        if (op == 0) *address = update;
        else scatter_fold_f64(address, update, op);
    }
};
SCATTER(double, scatter_f64)
