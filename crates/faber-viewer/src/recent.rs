use std::{
    fs, io,
    path::{Path, PathBuf},
};

type Entries = Vec<(String, PathBuf)>;

pub fn location() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base =
        std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Application Support"));
    #[cfg(not(any(windows, target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")));
    Some(base?.join("Faber").join("recent-projects.json"))
}

pub fn read(file: &Path) -> io::Result<Entries> {
    match fs::File::open(file) {
        Ok(reader) => Ok(serde_json::from_reader(reader)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

pub fn record(file: &Path, name: &str, root: &Path) -> io::Result<Entries> {
    let parent = file.parent().ok_or(io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(parent)?;
    let lock = fs::File::options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(file.with_extension("lock"))?;
    lock.lock()?;
    let path = std::path::absolute(root)?;
    let mut entries = read(file)?;
    entries.retain(|(_, saved)| *saved != path);
    entries.insert(0, (name.to_owned(), path));
    entries.truncate(8);
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(temp.as_file_mut(), &entries)?;
    temp.persist(file)?;
    Ok(entries)
}
