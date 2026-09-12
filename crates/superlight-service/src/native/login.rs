use std::io;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_paths_cannot_inject_plist_nodes() {
        assert_eq!(xml("a&<b>\"c'"), "a&amp;&lt;b&gt;&quot;c&apos;");
        assert!(checked_executable(std::path::Path::new("app\nextra")).is_err());
        assert_eq!(
            checked_executable(std::path::Path::new("App With Spaces")).unwrap(),
            "App With Spaces"
        );
    }
}
