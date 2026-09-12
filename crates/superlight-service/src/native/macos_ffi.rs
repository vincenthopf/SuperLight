#![allow(unsafe_op_in_unsafe_fn)]

use std::{ffi::{CStr, c_char, c_int, c_void}, io, ptr};

pub type Cf = *const c_void;
pub type Id = *mut c_void;
pub type Sel = *const c_void;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Point { pub x: f64, pub y: f64 }

#[repr(C)]
pub struct SourceContext {
    pub version: isize,
    pub info: *mut c_void,
    pub retain: Option<unsafe extern "C" fn(*const c_void) -> *const c_void>,
    pub release: Option<unsafe extern "C" fn(*const c_void)>,
    pub copy_description: Option<unsafe extern "C" fn(*const c_void) -> Cf>,
    pub equal: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> u8>,
    pub hash: Option<unsafe extern "C" fn(*const c_void) -> usize>,
    pub schedule: Option<unsafe extern "C" fn(*mut c_void, Cf, Cf)>,
    pub cancel: Option<unsafe extern "C" fn(*mut c_void, Cf, Cf)>,
    pub perform: Option<unsafe extern "C" fn(*mut c_void)>,
}

pub type TapCallback = unsafe extern "C" fn(Cf, u32, Cf, *mut c_void) -> Cf;
pub type InputCallback = unsafe extern "C" fn(*mut c_void, i32, Cf, u32, u32, *mut u8, isize);
pub type RemovalCallback = unsafe extern "C" fn(*mut c_void, i32, Cf);

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub static kCFRunLoopDefaultMode: Cf;
    pub static kCFRunLoopCommonModes: Cf;
    pub static kCFBooleanTrue: Cf;
    pub static kCFTypeDictionaryKeyCallBacks: u8;
    pub static kCFTypeDictionaryValueCallBacks: u8;
    pub fn CFRetain(value: Cf) -> Cf;
    pub fn CFRelease(value: Cf);
    pub fn CFGetTypeID(value: Cf) -> usize;
    pub fn CFStringGetTypeID() -> usize;
    pub fn CFNumberGetTypeID() -> usize;
    pub fn CFStringCreateWithCString(allocator: Cf, value: *const c_char, encoding: u32) -> Cf;
    pub fn CFStringGetCString(value: Cf, buffer: *mut c_char, length: isize, encoding: u32) -> u8;
    pub fn CFNumberCreate(allocator: Cf, kind: i32, value: *const c_void) -> Cf;
    pub fn CFNumberGetValue(number: Cf, kind: i32, value: *mut c_void) -> u8;
    pub fn CFDictionaryCreateMutable(allocator: Cf, capacity: isize, keys: Cf, values: Cf) -> Cf;
    pub fn CFDictionarySetValue(dictionary: Cf, key: Cf, value: Cf);
    pub fn CFDictionaryGetValue(dictionary: Cf, key: Cf) -> Cf;
    pub fn CFSetGetCount(set: Cf) -> isize;
    pub fn CFSetGetValues(set: Cf, values: *mut Cf);
    pub fn CFRunLoopGetCurrent() -> Cf;
    pub fn CFRunLoopGetMain() -> Cf;
    pub fn CFRunLoopRunInMode(mode: Cf, seconds: f64, return_after_source: u8) -> i32;
    pub fn CFRunLoopAddSource(loop_ref: Cf, source: Cf, mode: Cf);
    pub fn CFRunLoopRemoveSource(loop_ref: Cf, source: Cf, mode: Cf);
    pub fn CFRunLoopSourceCreate(allocator: Cf, order: isize, context: *mut SourceContext) -> Cf;
    pub fn CFRunLoopSourceSignal(source: Cf);
    pub fn CFRunLoopSourceInvalidate(source: Cf);
    pub fn CFRunLoopWakeUp(loop_ref: Cf);
    pub fn CFMachPortCreateRunLoopSource(allocator: Cf, port: Cf, order: isize) -> Cf;
    pub fn CFMachPortInvalidate(port: Cf);
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    pub fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, callback: TapCallback, user: *mut c_void) -> Cf;
    pub fn CGEventTapEnable(tap: Cf, enabled: u8);
    pub fn CGEventCreate(source: Cf) -> Cf;
    pub fn CGEventCreateMouseEvent(source: Cf, kind: u32, point: Point, button: u32) -> Cf;
    pub fn CGEventCreateKeyboardEvent(source: Cf, key: u16, down: u8) -> Cf;
    pub fn CGEventCreateScrollWheelEvent(source: Cf, units: u32, count: u32, ...) -> Cf;
    pub fn CGEventGetLocation(event: Cf) -> Point;
    pub fn CGEventGetIntegerValueField(event: Cf, field: u32) -> i64;
    pub fn CGEventGetDoubleValueField(event: Cf, field: u32) -> f64;
    pub fn CGEventSetIntegerValueField(event: Cf, field: u32, value: i64);
    pub fn CGEventSetDoubleValueField(event: Cf, field: u32, value: f64);
    pub fn CGEventSetFlags(event: Cf, flags: u64);
    pub fn CGEventSetType(event: Cf, kind: u32);
    pub fn CGEventPost(tap: u32, event: Cf);
    pub fn CGPreflightListenEventAccess() -> bool;
    pub fn CGPreflightPostEventAccess() -> bool;
    pub fn CGRequestListenEventAccess() -> bool;
    pub fn CGRequestPostEventAccess() -> bool;
    pub fn AXIsProcessTrusted() -> bool;
    pub fn AXIsProcessTrustedWithOptions(options: Cf) -> bool;
    pub static kAXTrustedCheckOptionPrompt: Cf;
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" { pub fn IsSecureEventInputEnabled() -> bool; }

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    pub fn IOHIDManagerCreate(allocator: Cf, options: u32) -> Cf;
    pub fn IOHIDManagerSetDeviceMatching(manager: Cf, dictionary: Cf);
    pub fn IOHIDManagerOpen(manager: Cf, options: u32) -> i32;
    pub fn IOHIDManagerClose(manager: Cf, options: u32) -> i32;
    pub fn IOHIDManagerCopyDevices(manager: Cf) -> Cf;
    pub fn IOHIDDeviceOpen(device: Cf, options: u32) -> i32;
    pub fn IOHIDDeviceClose(device: Cf, options: u32) -> i32;
    pub fn IOHIDDeviceGetProperty(device: Cf, key: Cf) -> Cf;
    pub fn IOHIDDeviceGetService(device: Cf) -> u32;
    pub fn IORegistryEntryGetRegistryEntryID(entry: u32, identifier: *mut u64) -> i32;
    pub fn IOHIDDeviceScheduleWithRunLoop(device: Cf, loop_ref: Cf, mode: Cf);
    pub fn IOHIDDeviceUnscheduleFromRunLoop(device: Cf, loop_ref: Cf, mode: Cf);
    pub fn IOHIDDeviceRegisterInputReportCallback(device: Cf, buffer: *mut u8, length: isize, callback: InputCallback, context: *mut c_void);
    pub fn IOHIDDeviceRegisterRemovalCallback(device: Cf, callback: RemovalCallback, context: *mut c_void);
    pub fn IOHIDDeviceSetReport(device: Cf, kind: u32, report_id: isize, report: *const u8, length: isize) -> i32;
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn objc_allocateClassPair(superclass: Id, name: *const c_char, extra: usize) -> Id;
    fn objc_registerClassPair(class: Id);
    fn class_addMethod(class: Id, selector: Sel, implementation: *const c_void, types: *const c_char) -> bool;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}

pub struct Owned(pub Cf);

impl Owned {
    pub fn new(value: Cf) -> io::Result<Self> {
        if value.is_null() { Err(io::Error::other("The macOS framework could not allocate an object")) } else { Ok(Self(value)) }
    }

    pub unsafe fn retained(value: Cf) -> io::Result<Self> {
        if value.is_null() { return Err(io::Error::other("Missing macOS framework object")); }
        Self::new(CFRetain(value))
    }
}

impl Drop for Owned { fn drop(&mut self) { unsafe { CFRelease(self.0); } } }

pub struct Pool(*mut c_void);
impl Pool { pub fn new() -> Self { Self(unsafe { objc_autoreleasePoolPush() }) } }
impl Drop for Pool { fn drop(&mut self) { unsafe { objc_autoreleasePoolPop(self.0); } } }

pub fn string(text: &str) -> io::Result<Owned> {
    let text = std::ffi::CString::new(text).map_err(io::Error::other)?;
    Owned::new(unsafe { CFStringCreateWithCString(ptr::null(), text.as_ptr(), 0x08000100) })
}

pub fn number(value: i32) -> io::Result<Owned> {
    Owned::new(unsafe { CFNumberCreate(ptr::null(), 3, (&value as *const i32).cast()) })
}

pub fn dictionary() -> io::Result<Owned> {
    Owned::new(unsafe { CFDictionaryCreateMutable(ptr::null(), 0, (&raw const kCFTypeDictionaryKeyCallBacks).cast(), (&raw const kCFTypeDictionaryValueCallBacks).cast()) })
}

pub unsafe fn text(value: Cf) -> String {
    if value.is_null() || CFGetTypeID(value) != CFStringGetTypeID() { return String::new(); }
    let mut buffer = [0u8; 4096];
    if CFStringGetCString(value, buffer.as_mut_ptr().cast(), buffer.len() as isize, 0x08000100) == 0 { return String::new(); }
    let length = buffer.iter().position(|byte| *byte == 0).unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..length]).into_owned()
}

pub unsafe fn integer(value: Cf) -> i32 {
    let mut result = 0i32;
    if !value.is_null() && CFGetTypeID(value) == CFNumberGetTypeID() { CFNumberGetValue(value, 3, (&raw mut result).cast()); }
    result
}

pub unsafe fn class(name: &CStr) -> Id { objc_getClass(name.as_ptr()) }
pub unsafe fn selector(name: &CStr) -> Sel { sel_registerName(name.as_ptr()) }

pub unsafe fn msg0<R>(object: Id, name: &CStr) -> R {
    let send: unsafe extern "C" fn(Id, Sel) -> R = std::mem::transmute(objc_msgSend as *const ());
    send(object, selector(name))
}

pub unsafe fn msg1<A, R>(object: Id, name: &CStr, a: A) -> R {
    let send: unsafe extern "C" fn(Id, Sel, A) -> R = std::mem::transmute(objc_msgSend as *const ());
    send(object, selector(name), a)
}

pub unsafe fn msg2<A, B, R>(object: Id, name: &CStr, a: A, b: B) -> R {
    let send: unsafe extern "C" fn(Id, Sel, A, B) -> R = std::mem::transmute(objc_msgSend as *const ());
    send(object, selector(name), a, b)
}

pub unsafe fn msg3<A, B, C, R>(object: Id, name: &CStr, a: A, b: B, c: C) -> R {
    let send: unsafe extern "C" fn(Id, Sel, A, B, C) -> R = std::mem::transmute(objc_msgSend as *const ());
    send(object, selector(name), a, b, c)
}

pub unsafe fn msg4<A, B, C, D, R>(object: Id, name: &CStr, a: A, b: B, c: C, d: D) -> R {
    let send: unsafe extern "C" fn(Id, Sel, A, B, C, D) -> R = std::mem::transmute(objc_msgSend as *const ());
    send(object, selector(name), a, b, c, d)
}

pub unsafe fn event(kind: usize, flags: usize, subtype: i16, data1: isize, data2: isize) -> Id {
    let send: unsafe extern "C" fn(Id, Sel, usize, Point, usize, f64, isize, Id, i16, isize, isize) -> Id = std::mem::transmute(objc_msgSend as *const ());
    send(class(c"NSEvent"), selector(c"otherEventWithType:location:modifierFlags:timestamp:windowNumber:context:subtype:data1:data2:"), kind, Point::default(), flags, 0.0, 0, ptr::null_mut(), subtype, data1, data2)
}

pub unsafe fn register_class(name: &CStr, methods: &[(&CStr, *const c_void)]) -> io::Result<Id> {
    let existing = class(name);
    if !existing.is_null() { return Ok(existing); }
    let class = objc_allocateClassPair(class(c"NSObject"), name.as_ptr(), 0);
    if class.is_null() { return Err(io::Error::other("Could not register the native application callbacks")); }
    for (name, implementation) in methods {
        if !class_addMethod(class, selector(name), *implementation, c"v@:@".as_ptr()) {
            return Err(io::Error::other("Could not register a native application callback"));
        }
    }
    objc_registerClassPair(class);
    Ok(class)
}

pub unsafe fn symbol(name: &CStr) -> *mut c_void { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) }

pub fn os_error(operation: &str, code: c_int) -> io::Error { io::Error::other(format!("{operation} failed: 0x{:08x}", code as u32)) }
