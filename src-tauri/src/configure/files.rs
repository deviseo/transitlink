//! 配置文件的读、备份和写。
//!
//! - 写入先写同目录的临时文件再改名，中途失败不会留下写了一半的配置。
//! - 配置文件是符号链接时（例如用 dotfiles 管理），写到它指向的真实文件，不破坏链接。
//! - 每次修改前把原文件复制到 `~/.transitlink/backups/<工具>/`，最多留 [`KEEP_BACKUPS`] 份；
//!   第一次修改前的原文件另存为 `original-<文件名>`，一直保留。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// 每个工具最多保留多少份普通备份（不含 `original-*`）。
const KEEP_BACKUPS: usize = 20;
const ORIGINAL_PREFIX: &str = "original-";

/// 读文本文件；不存在时返回 None。
pub fn read(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".to_string())
}

/// 修改前备份：复制到 `dir/<时间>-<文件名>`，并清理多余的旧备份。文件不存在时不备份。
/// `keep_original` 为真且还没有 `original-<文件名>` 时，再另存一份原文件。
pub fn backup(
    path: &Path,
    dir: &Path,
    now: u64,
    keep_original: bool,
) -> io::Result<Option<PathBuf>> {
    if !path.is_file() {
        return Ok(None);
    }
    create_private_dir(dir)?;
    let name = file_name(path);
    if keep_original {
        let original = dir.join(format!("{ORIGINAL_PREFIX}{name}"));
        if !original.exists() {
            copy_private(path, &original)?;
        }
    }
    let stamp = crate::clock::stamp(now);
    let mut target = dir.join(format!("{stamp}-{name}"));
    let mut counter = 1;
    while target.exists() {
        target = dir.join(format!("{stamp}-{counter}-{name}"));
        counter += 1;
    }
    copy_private(path, &target)?;
    prune(dir)?;
    Ok(Some(target))
}

fn prune(dir: &Path) -> io::Result<()> {
    let mut backups: Vec<PathBuf> = fs::read_dir(dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && !path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(ORIGINAL_PREFIX))
        })
        .collect();
    // 文件名以时间开头，按名字排就是按时间排。
    backups.sort();
    let extra = backups.len().saturating_sub(KEEP_BACKUPS);
    for path in backups.into_iter().take(extra) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

/// 原子写入。已有文件保留原来的权限；新建的文件只有自己能读写（里面有 Key）。
pub fn write(path: &Path, contents: &str) -> io::Result<()> {
    let target = real_path(path)?;
    let dir = target
        .parent()
        .ok_or_else(|| io::Error::other("config path has no parent directory"))?;
    fs::create_dir_all(dir)?;
    let temp = dir.join(format!(".{}.transitlink-tmp", file_name(&target)));
    fs::write(&temp, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&target)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .unwrap_or(0o600);
        fs::set_permissions(&temp, fs::Permissions::from_mode(mode))?;
    }
    fs::rename(&temp, &target).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

/// 删除配置文件（还原时，文件本来不存在、我们写进去的又都撤掉了）。
pub fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(real_path(path)?) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// 符号链接指向的真实文件；不是链接时就是它自己。
fn real_path(path: &Path) -> io::Result<PathBuf> {
    if path.is_symlink() {
        fs::canonicalize(path)
    } else {
        Ok(path.to_path_buf())
    }
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// 备份里有 Key，只让自己能读。
fn copy_private(from: &Path, to: &Path) -> io::Result<()> {
    fs::copy(from, to)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(to, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("transitlink-files-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_atomically_and_keeps_permissions() {
        let dir = temp_dir("write");
        let path = dir.join("nested/settings.json");
        assert_eq!(read(&path).unwrap(), None);
        write(&path, "{}\n").unwrap();
        assert_eq!(read(&path).unwrap().as_deref(), Some("{}\n"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600, "new files are private");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            write(&path, "{\"a\":1}\n").unwrap();
            assert_eq!(mode(&path), 0o644, "existing permissions are kept");
        }
        assert!(
            !dir.join("nested/.settings.json.transitlink-tmp").exists(),
            "temp file is renamed away"
        );
        remove(&path).unwrap();
        remove(&path).unwrap();
        assert_eq!(read(&path).unwrap(), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_symlinks() {
        let dir = temp_dir("symlink");
        let real = dir.join("dotfiles/config.toml");
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::write(&real, "a = 1\n").unwrap();
        let link = dir.join("config.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        write(&link, "a = 2\n").unwrap();
        assert!(link.is_symlink(), "the link survives");
        assert_eq!(fs::read_to_string(&real).unwrap(), "a = 2\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn backups_keep_the_original_and_a_bounded_history() {
        let dir = temp_dir("backup");
        let path = dir.join("config.toml");
        let backups = dir.join("backups");
        assert_eq!(
            backup(&path, &backups, 0, true).unwrap(),
            None,
            "nothing to back up"
        );

        fs::write(&path, "v = 0\n").unwrap();
        backup(&path, &backups, 1_000, true).unwrap();
        for i in 1..=(KEEP_BACKUPS + 5) {
            fs::write(&path, format!("v = {i}\n")).unwrap();
            backup(&path, &backups, 1_000 + i as u64, true).unwrap();
        }
        let names: Vec<String> = fs::read_dir(&backups)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), KEEP_BACKUPS + 1);
        assert_eq!(
            fs::read_to_string(backups.join("original-config.toml")).unwrap(),
            "v = 0\n",
            "the first version is kept for good"
        );

        // 同一秒内的两次备份不会互相覆盖。
        let again = backup(&path, &backups, 1_000 + (KEEP_BACKUPS + 5) as u64, false)
            .unwrap()
            .unwrap();
        assert!(again.file_name().unwrap().to_string_lossy().contains("-1-"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
