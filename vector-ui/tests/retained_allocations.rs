use forma::{display_list::RenderScene, Button};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}

struct CountingAllocator;
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn allocated() {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(value) = count.get() {
            count.set(Some(value + 1));
        }
    });
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() { allocated(); }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc_zeroed(layout);
        if !ptr.is_null() { allocated(); }
        ptr
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let ptr = System.realloc(ptr, layout, size);
        if !ptr.is_null() { allocated(); }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }
}

#[test]
fn native_cpu_animation_and_gpu_input_serialization_reuse_allocations() {
    let mut model = Button::new();
    let mut pixels = vec![0; 400 * 200];
    let mut params = Vec::new();
    let mut paints = Vec::new();
    model.paint_native(&mut pixels, 400, 200, 1.).unwrap();
    model.gpu_params_into(400, 200, 1., true, &mut params);
    model.gpu_paints_into(&mut paints);
    let initial = pixels.clone();

    ALLOCATIONS.with(|count| count.set(Some(0)));
    model.pointer(100., 90., 0);
    model.tick(1000.);
    model.paint_native(&mut pixels, 400, 200, 1.).unwrap();
    model.gpu_params_into(400, 200, 1., true, &mut params);
    model.gpu_paints_into(&mut paints);
    let allocations = ALLOCATIONS.with(|count| count.replace(None).unwrap());

    assert_ne!(pixels, initial, "the measured frame must actually repaint");
    assert_eq!(allocations, 0, "warmed native repaint and serialization must retain buffers");
    assert_eq!(params, model.gpu_params(400, 200, 1., true));
    assert_eq!(paints, model.gpu_paints());
}
