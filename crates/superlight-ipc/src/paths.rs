use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub runtime: PathBuf,
    pub endpoint: PathBuf,
    pub legacy: Option<PathBuf>,
}

impl Paths {
    pub fn in_dir(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let runtime = root.join("run");
        Self {
            config: root.join("config.json"),
            endpoint: runtime.join("endpoint.json"),
            legacy: None,
            root,
            runtime,
        }
    }

    pub fn discover() -> io::Result<Self> {
        if let Some(root) = env::var_os("SUPERLIGHT_CONFIG_DIR") {
            return Ok(Self::in_dir(root));
        }
        let base = configuration_base()?;
        let mut paths = Self::in_dir(base.join("SuperLight"));
        paths.legacy = Some(base.join("Mouser").join("config.json"));
        Ok(paths)
    }

    pub fn prepare(&self) -> io::Result<()> {
        private_directory(&self.root)?;
        private_directory(&self.runtime)
    }
}

pub fn home() -> io::Result<PathBuf> {
    let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "No absolute home directory is available",
            )
        })
}

pub fn configuration_base() -> io::Result<PathBuf> {
    if cfg!(target_os = "macos") {
        Ok(home()?.join("Library").join("Application Support"))
    } else if cfg!(windows) {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "APPDATA is unavailable"))
    } else {
        Ok(env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or(home()?.join(".config")))
    }
}

pub fn private_directory(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Application directories must be real directories, not symbolic links",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o022 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Application directories must not be writable by other users",
            ));
        }
    }
    Ok(())
}
