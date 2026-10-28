// Direct channel-first grouped cross-correlation. No im2col or widened operand
// arrays: each thread accumulates one output in f32, then writes it once.
// Runtime NVRTC options disable FTZ and contraction, including for decoded BF16
// subnormal operands whose product is normal. Layout addresses remain u64.
struct ConvFloatLoad {
    __device__ float operator()(float value) const { return value; }
};
struct ConvLowLoad {
    unsigned dtype;
    __device__ float operator()(unsigned short value) const {
        return decode_low(value, dtype);
    }
};
template <typename T, typename Load>
__device__ void convolution_impl(const T* input, const T* weight, float* output,
    const U64* m, U64 count, Load load) {
    const unsigned rank = (unsigned)m[0];
    const U64 channels = m[1], outputs_per_group = m[2], kernel_count = m[3];
    const U64* input_dims = m + 11;
    const U64* output_dims = input_dims + rank;
    const U64* kernel_dims = output_dims + rank;
    const U64* input_strides = kernel_dims + rank;
    const U64* weight_strides = input_strides + rank;
    const U64* strides = weight_strides + rank;
    const U64* dilations = strides + rank;
    const U64* padding = dilations + rank;
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 linear = (U64)blockIdx.x * blockDim.x + threadIdx.x;
         linear < count; linear += step) {
        U64 coordinate[3];
        U64 remaining = linear;
        for (unsigned axis = rank; axis > 0; --axis) {
            coordinate[axis - 1] = remaining % output_dims[axis - 1];
            remaining /= output_dims[axis - 1];
        }
        const U64 channel_out = remaining % m[10];
        const U64 batch = remaining / m[10];
        const U64 group = channel_out / outputs_per_group;
        const U64 input_base = m[4] + batch * m[6] + group * channels * m[7];
        const U64 weight_base = m[5] + channel_out * m[8];
        float total = 0.0f;
        for (U64 channel = 0; channel < channels; ++channel) {
            for (U64 kernel = 0; kernel < kernel_count; ++kernel) {
                U64 kernel_remaining = kernel;
                U64 ia = input_base + channel * m[7];
                U64 iw = weight_base + channel * m[9];
                bool valid = true;
                for (unsigned axis = rank; axis > 0; --axis) {
                    const unsigned d = axis - 1;
                    const U64 kernel_coordinate = kernel_remaining % kernel_dims[d];
                    kernel_remaining /= kernel_dims[d];
                    const U64 padded = coordinate[d] * strides[d] + kernel_coordinate * dilations[d];
                    // Check before subtracting zero padding or reading input.
                    if (padded < padding[d] || padded - padding[d] >= input_dims[d]) {
                        valid = false;
                        break;
                    }
                    ia += (padded - padding[d]) * input_strides[d];
                    iw += kernel_coordinate * weight_strides[d];
                }
                if (valid) total += load(input[ia]) * load(weight[iw]);
            }
        }
        output[linear] = total;
    }
}
extern "C" __global__ void conv_f32(const float* input, const float* weight,
    float* output, const U64* metadata, U64 count) {
    convolution_impl(input, weight, output, metadata, count, ConvFloatLoad{});
}
extern "C" __global__ void conv_low(const unsigned short* input, const unsigned short* weight,
    float* output, const U64* metadata, U64 count, unsigned dtype) {
    convolution_impl(input, weight, output, metadata, count, ConvLowLoad{dtype});
}
