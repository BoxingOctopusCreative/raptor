use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;

#[cfg(unix)]
const EXDEV: i32 = 18;
#[cfg(windows)]
const EXDEV: i32 = 17;

/// Move `src` to `dest`, copying when rename fails across mount points (EXDEV).
pub fn move_file(src: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(err) if err.raw_os_error() == Some(EXDEV) => {
            fs::copy(src, dest)?;
            fs::remove_file(src)?;
            Ok(())
        }
        Err(err) => Err(err.into()),
    }
}

pub fn temp_file_in(dir: &Path, label: &str) -> Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let safe_label: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok(dir.join(format!(".raptor-{safe_label}-{nanos}.part")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_file_within_same_directory() {
        let dir = std::env::temp_dir().join(format!("raptor-move-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let src = dir.join("src.txt");
        let dest = dir.join("dest.txt");
        fs::write(&src, b"payload").unwrap();

        move_file(&src, &dest).unwrap();
        assert!(!src.exists());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "payload");

        let _ = fs::remove_dir_all(&dir);
    }
}
