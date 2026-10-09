use std::io;

#[cfg(target_os = "macos")]
pub fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn checked_executable(path: &std::path::Path) -> io::Result<&str> {
    let value = path
        .to_str()
        .ok_or_else(|| io::Error::other("The application path is not valid Unicode"))?;
    if value.contains(['\n', '\r', '\0']) {
        return Err(io::Error::other(
            "The application path contains unsupported control characters",
        ));
    }
    Ok(value)
}
