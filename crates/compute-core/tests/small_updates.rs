#![cfg(not(target_arch = "wasm32"))]
use compute_core::{ComputeRuntime, ScratchPool, gpu_compute::GpuContext};
use std::time::Duration;
const WAIT: Duration = Duration::from_secs(10);
fn context() -> Option<GpuContext> {
    let c = GpuContext::new();
    assert!(c.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
    c
}
#[test]
fn coalesced_parts_preserve_offsets_aliases_bits_and_snapshot_order() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    let a = rt.upload(&[9u32; 1026]).unwrap();
    let values: Vec<u32> = (0..1024)
        .map(|i| 0x7f800001u32.wrapping_add(i * 7919))
        .collect();
    rt.write_small_parts(&a, 1, &[&values[..1], &[], &values[1..512], &values[512..]])
        .unwrap();
    let first = rt.read(&a).unwrap();
    let alias = a.clone();
    rt.write_small_parts(&alias, 2, &[&[42], &[43, 44]])
        .unwrap();
    let second = rt.read(&a).unwrap();
    let mut expected = vec![9];
    expected.extend_from_slice(&values);
    expected.push(9);
    let mut changed = expected.clone();
    changed[2..5].copy_from_slice(&[42, 43, 44]);
    assert_eq!(second.wait(WAIT).unwrap(), changed);
    assert_eq!(first.wait(WAIT).unwrap(), expected);
    // All errors occur before any update, including an oversized valid range.
    assert!(rt.write_small_parts(&a, 0, &[&values, &[0]]).is_err());
    assert!(rt.write_small_parts(&a, usize::MAX, &[&[0]]).is_err());
    assert!(rt.write_small_parts(&a, 1026, &[&[0]]).is_err());
    let foreign = ComputeRuntime::new(&c).unwrap().zeros::<u32>(2).unwrap();
    assert!(rt.write_small_parts(&foreign, 0, &[&[0]]).is_err());
    rt.write_small_parts(&a, 1026, &[&[], &[]]).unwrap();
    assert_eq!(rt.read(&a).unwrap().wait(WAIT).unwrap(), changed);
    // The borrowed single-part path preserves every f32 bit pattern too.
    let bits = [0x80000000, 0x7fc01234, 0x7f800000, 0xff800000];
    let f = rt.zeros::<f32>(4).unwrap();
    let floats = bits.map(f32::from_bits);
    rt.write_small_parts(&f, 0, &[&[], &floats]).unwrap();
    assert_eq!(
        rt.read(&f)
            .unwrap()
            .wait(WAIT)
            .unwrap()
            .into_iter()
            .map(f32::to_bits)
            .collect::<Vec<_>>(),
        bits
    );
}
#[test]
fn scratch_reuse_needs_no_gpu_wait_and_does_not_overwrite_pending_snapshot() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    let mut pool = ScratchPool::new(&rt, 4096);
    let first = pool.reserve::<u32>("small", 1024).unwrap();
    rt.write_small_parts(&first.array, 0, &[&[11; 512], &[12; 512]])
        .unwrap();
    let pending = rt.read(&first.array).unwrap();
    let next = pool.reserve::<u32>("small", 1024).unwrap();
    assert_eq!(first.generation, next.generation);
    assert_eq!(first.array.view().raw(), next.array.view().raw());
    assert_eq!(pool.reserved_bytes(), 4096);
    rt.write_small_parts(&next.array, 0, &[&[21; 512], &[22; 512]])
        .unwrap();
    let newer = rt.read(&next.array).unwrap();
    assert_eq!(
        newer.wait(WAIT).unwrap(),
        [vec![21; 512], vec![22; 512]].concat()
    );
    assert_eq!(
        pending.wait(WAIT).unwrap(),
        [vec![11; 512], vec![12; 512]].concat()
    );
}
