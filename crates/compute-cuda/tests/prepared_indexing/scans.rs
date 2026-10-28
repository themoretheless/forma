use super::*;

fn prefix<T: Copy>(input: &[T], options: ScanOptions, zero: T, add: impl Fn(T, T) -> T) -> Vec<T> {
    let mut output = vec![zero; input.len()];
    let mut sum = zero;
    for position in 0..input.len() {
        let index = if options.reverse {
            input.len() - 1 - position
        } else {
            position
        };
        if options.inclusive {
            sum = add(sum, input[index]);
            output[index] = sum;
        } else {
            output[index] = sum;
            sum = add(sum, input[index]);
        }
    }
    output
}
pub fn float_and_unsigned(rt: &CudaRuntime) -> Result {
    let n = 131077;
    let values: Vec<_> = (0..n).map(|i| (i % 3) as f32 - 1.).collect();
    let input = rt.upload_f32(shape(&[n]), &values)?;
    let mut graph = rt.program();
    let x = graph.input(input.layout().clone())?;
    let outputs = modes()
        .map(|options| graph.scan(x, 0, options))
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    let mut program = graph.prepare(&outputs, CudaPrepareOptions::default())?;
    for _ in 0..2 {
        let result = program.run(&[&input])?;
        for (i, mode) in modes().into_iter().enumerate() {
            let expected = prefix(
                &values.iter().map(|&x| f64::from(x)).collect::<Vec<_>>(),
                mode,
                0.,
                |a, b| a + b,
            )
            .into_iter()
            .map(|x| x as f32)
            .collect::<Vec<_>>();
            assert_eq!(rt.read_f32(&result[i])?, expected);
        }
    }
    // Axis one is strided, and all additions must wrap as u32, including carries.
    for length in [257, 65537] {
        let versions = [0u32, 17].map(|bias| {
            (0..length * 6)
                .map(|i| {
                    u32::MAX
                        .wrapping_sub((i as u32).wrapping_mul(3))
                        .wrapping_add(bias)
                })
                .collect::<Vec<_>>()
        });
        let input0 = rt.upload_u32(shape(&[length, 3, 2]), &versions[0])?;
        let input1 = rt.upload_u32(shape(&[length, 3, 2]), &versions[1])?;
        let a = rt.permute_u32(&input0, &[1, 0, 2])?;
        let b = rt.permute_u32(&input1, &[1, 0, 2])?;
        let mut graph = rt.program();
        let x = graph.input_u32(a.layout().clone())?;
        let outputs = modes()
            .map(|options| graph.scan_u32(x, 1, options))
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        let mut program = graph.prepare(&outputs, CudaPrepareOptions::default())?;
        let mut result = program.run_typed(&[(&a).into()])?;
        for version in [1, 0, 1] {
            replay(
                &mut program,
                &[if version == 0 {
                    (&a).into()
                } else {
                    (&b).into()
                }],
                &mut result,
            )?;
            for (mode_index, mode) in modes().into_iter().enumerate() {
                let mut expected = vec![0; length * 6];
                for row in 0..3 {
                    for inner in 0..2 {
                        let logical = (0..length)
                            .map(|column| versions[version][(column * 3 + row) * 2 + inner])
                            .collect::<Vec<_>>();
                        let scanned = prefix(&logical, mode, 0, u32::wrapping_add);
                        for column in 0..length {
                            expected[(row * length + column) * 2 + inner] = scanned[column];
                        }
                    }
                }
                assert_eq!(rt.read_u32(result[mode_index].as_u32()?)?, expected);
            }
        }
    }
    Ok(())
}
pub fn low(rt: &CudaRuntime, dtype: LowDtype) -> Result {
    let one = match dtype {
        LowDtype::F16 => 0x3c00u16,
        LowDtype::Bf16 => 0x3f80,
    };
    let n = 65537;
    let bits: Vec<_> = (0..n)
        .map(|i| if i % 2 == 0 { one } else { one | 0x8000 })
        .collect();
    let input = rt.upload_low(dtype, shape(&[n]), &bits)?;
    let mut graph = rt.program();
    let x = graph.input_low(dtype, input.layout().clone())?;
    let mut outputs = Vec::new();
    for mode in modes() {
        outputs.push(graph.scan_low_f32(x, 0, mode)?);
        outputs.push(graph.scan_low(x, 0, mode)?);
    }
    let mut program = graph.prepare(&outputs, CudaPrepareOptions::default())?;
    let values = (0..n)
        .map(|i| if i % 2 == 0 { 1. } else { -1. })
        .collect::<Vec<f64>>();
    for _ in 0..2 {
        let result = program.run_typed(&[(&input).into()])?;
        for (i, mode) in modes().into_iter().enumerate() {
            let expected = prefix(&values, mode, 0., |a, b| a + b);
            assert_eq!(
                rt.read_f32(result[2 * i].as_f32()?)?,
                expected.iter().map(|&x| x as f32).collect::<Vec<_>>()
            );
            let raw = expected
                .iter()
                .map(|&x| {
                    if x == 0. {
                        0
                    } else if x == 1. {
                        one
                    } else {
                        assert_eq!(x, -1.);
                        one | 0x8000
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(rt.read_low_bits(result[2 * i + 1].as_low()?)?, raw);
        }
    }
    // Prefixes above the low format's integer precision must stay f32 until
    // the final cast. A low accumulator or a rounded intermediate loses ones.
    let input = rt.upload_low(dtype, shape(&[4097]), &vec![one; 4097])?;
    let mut graph = rt.program();
    let x = graph.input_low(dtype, input.layout().clone())?;
    let wide = graph.scan_low_f32(x, 0, ScanOptions::default())?;
    let rounded = graph.scan_low(x, 0, ScanOptions::default())?;
    let mut program = graph.prepare(&[wide, rounded], CudaPrepareOptions::default())?;
    for _ in 0..2 {
        let result = program.run_typed(&[(&input).into()])?;
        assert_eq!(
            rt.read_f32(result[0].as_f32()?)?,
            (1..=4097).map(|x| x as f32).collect::<Vec<_>>()
        );
        assert_eq!(
            rt.read_low_bits(result[1].as_low()?)?.last().copied(),
            Some(match dtype {
                LowDtype::F16 => 0x6c00,
                LowDtype::Bf16 => 0x4580,
            })
        );
    }
    Ok(())
}
