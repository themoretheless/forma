//! Reuse a resident indexing pipeline with changed values, masks and indices.
use compute_mlx::MlxBackend;
use tensor_core::{BinaryOp, ScanOptions, ScatterOp, Shape};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = MlxBackend::new_gpu()?;
    let shape = Shape::new(vec![4])?;
    let mut graph = backend.program();
    let values = graph.input(shape.clone())?;
    let indices = graph.input_u32(shape.clone())?;
    let mask = graph.input_u32(shape.clone())?;
    let destinations = graph.input_u32(shape.clone())?;
    let gathered = graph.gather(values, indices, 0)?;
    let prefix = graph.scan(
        gathered.values,
        0,
        ScanOptions {
            inclusive: true,
            reverse: false,
        },
    )?;
    let selected = graph.compact(prefix, mask)?;
    let scattered = graph.scatter(values, destinations, selected.values, ScatterOp::Replace, 0)?;
    let invalid = graph.binary_u32(
        gathered.invalid_count,
        scattered.invalid_count,
        BinaryOp::Add,
    )?;
    let program = graph.compile(&[scattered.values, selected.count, invalid])?;

    for (values, indices, mask, destinations, expected, count, invalid) in [
        (
            [1., 2., 3., 4.],
            [3, 0, 99, 1],
            [1, 0, 2, 1],
            [0, 2, 2, 99],
            [4., 2., 7., 4.],
            3,
            2,
        ),
        (
            [10., 20., 30., 40.],
            [0, 1, 2, 3],
            [0; 4],
            [3, 2, 1, 0],
            [0.; 4],
            0,
            0,
        ),
    ] {
        let values = backend.upload_f32(shape.clone(), &values)?;
        let indices = backend.upload_u32(shape.clone(), &indices)?;
        let mask = backend.upload_u32(shape.clone(), &mask)?;
        let destinations = backend.upload_u32(shape.clone(), &destinations)?;
        let output = program.run_typed(&[
            (&values).into(),
            (&indices).into(),
            (&mask).into(),
            (&destinations).into(),
        ])?;
        // All intermediate arrays and both kinds of counters remain on the GPU.
        let actual = backend.read_f32(output[0].as_tensor()?)?;
        assert_eq!(actual, expected);
        assert_eq!(backend.read_u32(output[1].as_tensor()?)?, vec![count]);
        assert_eq!(backend.read_u32(output[2].as_tensor()?)?, vec![invalid]);
        println!("values={actual:?} selected={count} invalid={invalid}");
    }
    println!(
        "runtime={} traces={}",
        backend.version(),
        program.trace_count()
    );
    Ok(())
}
