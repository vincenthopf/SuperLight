use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box,
};
use superlight_core::{
    actions::Action,
    hidpp,
    input::Router,
    reports::{DeviceEvent, Notifications},
    session::{Divert, Report},
};

thread_local! {
    static MEASURING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct Allocator;

fn record_allocation() {
    let _ = MEASURING.try_with(|measuring| {
        if measuring.get() {
            let _ = ALLOCATIONS.try_with(|count| count.set(count.get().saturating_add(1)));
        }
    });
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        unsafe { System.realloc(pointer, layout, size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

#[test]
fn one_million_hid_edges_allocate_no_memory_after_initialization() {
    let mut decoder = Notifications::new(2, 7);
    decoder.configure([Some(Divert { cid: 0xc3, raw_xy: true }), None, None]);
    let mut router = Router::default();
    let mut wire = [0u8; 20];
    wire[..4].copy_from_slice(&[0x11, 2, 7, 0]);
    let mut emitted = 0usize;
    ALLOCATIONS.with(|count| count.set(0));
    MEASURING.with(|measuring| measuring.set(true));
    for index in 0..1_000_000 {
        wire[5] = if index % 2 == 0 { 0xc3 } else { 0 };
        let message = hidpp::parse(black_box(&wire)).unwrap();
        decoder.process(Report::from_message(message), |event| {
            if let DeviceEvent::Button { source, down } = event {
                let _ = router.route(usize::from(source), down, Action::Mouse(0), |_| {
                    emitted += 1;
                    true
                });
            }
        });
    }
    MEASURING.with(|measuring| measuring.set(false));
    let allocations = ALLOCATIONS.with(Cell::get);
    assert_eq!(emitted, 1_000_000);
    assert_eq!(allocations, 0);
}
