use serde_json::{Value, json};
use std::{
    ffi::{CString, c_char},
    panic::catch_unwind,
};
use superlight_core::{
    actions::{ACTIONS, Action, Platform},
    config, policy,
};
use superlight_ipc::{
    Paths, Request, Response,
    channel::{Endpoint, call_endpoint},
};

fn dispatch(bytes: &[u8], paths: &Paths) -> Result<Value, String> {
    if bytes.len() > superlight_core::CONFIG_LIMIT + 4096 {
        return Err("Request exceeds the size limit".into());
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    match value["local"].as_str() {
        Some("catalog") => {
            return Ok(
                json!({"ok": true, "actions": ACTIONS.iter().filter(|(id, _)| Action::parse(id, Platform::MacOs).is_ok()).collect::<Vec<_>>()}),
            );
        }
        Some("validate_action") => {
            Action::parse(
                value["action"].as_str().ok_or("Missing action")?,
                Platform::MacOs,
            )?;
            return Ok(json!({"ok": true}));
        }
        Some(_) => return Err("Unknown local operation".into()),
        None => {}
    }
    let request: Request =
        serde_json::from_value(value["request"].clone()).map_err(|e| e.to_string())?;
    if let Request::Apply {
        config: ref candidate,
        ..
    } = request
    {
        let bytes = serde_json::to_vec(candidate).map_err(|e| e.to_string())?;
        let config = config::parse(&bytes)?;
        policy::validate_actions(&config, Platform::MacOs)?;
        if value["expected_instance"].as_str().is_none() {
            return Err("Missing service instance for save".into());
        }
    }
    let endpoint = Endpoint::load(paths).map_err(|e| e.to_string())?;
    if let Some(expected) = value["expected_instance"].as_str()
        && endpoint.instance != expected
    {
        return Err("The service restarted. Reload before saving.".into());
    }
    let response = call_endpoint(&endpoint, &request).map_err(|e| e.to_string())?;
    serde_json::to_value(response).map_err(|e| e.to_string())
}

fn encode(result: Result<Value, String>) -> CString {
    let value = result.unwrap_or_else(|error| {
        serde_json::to_value(Response::failure(error)).expect("serializable response")
    });
    CString::new(value.to_string()).expect("JSON has no literal nul")
}

#[allow(clippy::missing_safety_doc)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn superlight_call(bytes: *const u8, length: usize) -> *mut c_char {
    let result = catch_unwind(|| {
        if bytes.is_null() || length > superlight_core::CONFIG_LIMIT + 4096 {
            return Err("Invalid request buffer".into());
        }
        let bytes = unsafe { std::slice::from_raw_parts(bytes, length) };
        let paths = Paths::discover().map_err(|e| e.to_string())?;
        dispatch(bytes, &paths)
    })
    .unwrap_or_else(|_| Err("Native IPC bridge failed".into()));
    encode(result).into_raw()
}

#[allow(clippy::missing_safety_doc)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn superlight_free(value: *mut c_char) {
    if !value.is_null() {
        drop(unsafe { CString::from_raw(value) });
    }
}
