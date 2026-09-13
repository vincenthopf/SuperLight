from pathlib import Path

path = Path("crates/superlight-service/src/native/windows.rs")
source = path.read_text()
source = source.replace("cell::RefCell,", "cell::{Cell, RefCell},")
source = source.replace(
    "thread_local! { static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) }; }",
    "thread_local! {\n    static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };\n    static LAST_POINT: Cell<Option<(i32, i32)>> = const { Cell::new(None) };\n}",
)
old = '''            WM_MOUSEMOVE => {
                let mut previous = POINT { x: 0, y: 0 };
                if unsafe { GetCursorPos(&mut previous) } == 0 {
                    return false;
                }
                hook.movement(
                    f64::from(event.pt.x.saturating_sub(previous.x)),
                    f64::from(event.pt.y.saturating_sub(previous.y)),
                )
            }
'''
new = '''            WM_MOUSEMOVE => movement_delta(event.pt.x, event.pt.y)
                .is_some_and(|(x, y)| hook.movement(x, y)),
'''
if old not in source:
    raise SystemExit("Windows movement block did not match the reviewed source")
source = source.replace(old, new)
needle = "fn process_mouse(kind: u32, event: &MSLLHOOKSTRUCT) -> bool {"
helper = '''fn movement_delta(x: i32, y: i32) -> Option<(f64, f64)> {
    LAST_POINT.with(|previous| {
        previous.replace(Some((x, y))).map(|(old_x, old_y)| {
            (
                f64::from(x.saturating_sub(old_x)),
                f64::from(y.saturating_sub(old_y)),
            )
        })
    })
}

'''
if needle not in source:
    raise SystemExit("Windows mouse hook entry point was not found")
source = source.replace(needle, helper + needle, 1)
source = source.replace(
    "        HOOK.with(|hook| *hook.borrow_mut() = None);",
    "        HOOK.with(|hook| *hook.borrow_mut() = None);\n        LAST_POINT.with(|point| point.set(None));",
    1,
)
source += '''

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_delta_uses_the_previous_hook_position() {
        LAST_POINT.with(|point| point.set(None));
        assert_eq!(movement_delta(10, 20), None);
        assert_eq!(movement_delta(15, 17), Some((5.0, -3.0)));
    }
}
'''
path.write_text(source)
