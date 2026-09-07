use forma::{display_list::RenderScene, Button, Runtime};
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

#[test]
fn multiple_controls_reuse_native_buffers_for_static_and_animated_frames() {
    let source = "component Demo { Frame { width:128; height:128; padding:8; gap:12; clip:true; radius:12; background:#112233; Button { key:'one'; width:100; height:40; } Button { key:'two'; width:100; height:40; } } }";
    let template = "component Button { Rectangle { radius:8; background:Brush { color:#324674cc; hover:#80aaffee; transition:180ms; }; Reveal { color:#ffaa77; } Text { text:'Forma Я'; fontSize:13; color:#ffffff; } PointerArea { clicked -> events.clicked(); } } }";
    let mut model = Runtime::from_sources(source, template).unwrap();
    for scale in [1., 1.25, 2.] {
        let side = (128. * scale) as u32;
        let mut pixels = vec![0; (side * side) as usize];
        model.paint_native(&mut pixels, side, side, scale).unwrap();
        let initial = pixels.clone();

        ALLOCATIONS.with(|count| count.set(Some(0)));
        model.paint_native(&mut pixels, side, side, scale).unwrap();
        let allocations = ALLOCATIONS.with(|count| count.replace(None).unwrap());
        assert_eq!(pixels, initial);
        assert_eq!(allocations, 0, "static multi-control frame must reuse buffers");

        ALLOCATIONS.with(|count| count.set(Some(0)));
        model.pointer(30., 25., 0);
        model.tick(16.);
        model.paint_native(&mut pixels, side, side, scale).unwrap();
        let allocations = ALLOCATIONS.with(|count| count.replace(None).unwrap());
        assert_ne!(pixels, initial, "measured animation must actually repaint");
        assert_eq!(allocations, 0, "animated multi-control frame must reuse buffers");
        model.pointer(-1., -1., 3);
        model.tick(1000.);
    }
}
