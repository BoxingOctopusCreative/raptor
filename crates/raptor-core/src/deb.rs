use std::collections::HashSet;
use std::fs;
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use ar::Archive;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use tar::{Archive as TarArchive, Builder as TarBuilder, Header};
use walkdir::WalkDir;
use xz2::read::XzDecoder;
use zstd::stream::read::Decoder as ZstdDecoder;

use crate::control::ControlFile;
use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct DebArchive {
    pub control: ControlFile,
    pub control_extra: Vec<(String, Vec<u8>)>,
    pub data_files: Vec<(String, Vec<u8>, u32)>,
}

pub fn read_deb(path: &Path) -> Result<DebArchive> {
    let file = File::open(path)?;
    let mut archive = Archive::new(file);
    let mut debian_binary = String::new();
    let mut control_bytes = Vec::new();
    let mut data_bytes = Vec::new();

    while let Some(entry) = archive.next_entry() {
        let mut entry = entry.map_err(|e| Error::InvalidDeb(e.to_string()))?;
        let name = String::from_utf8_lossy(entry.header().identifier())
            .trim()
            .to_string();

        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| Error::InvalidDeb(e.to_string()))?;

        match name.as_str() {
            "debian-binary" => debian_binary = String::from_utf8_lossy(&buf).trim().to_string(),
            n if n.starts_with("control.tar") => control_bytes = buf,
            n if n.starts_with("data.tar") => data_bytes = buf,
            _ => {}
        }
    }

    if debian_binary.is_empty() {
        return Err(Error::InvalidDeb("missing debian-binary member".into()));
    }

    let (control, control_extra) = extract_control_tar(&control_bytes)?;
    let data_files = extract_data_tar(&data_bytes)?;

    Ok(DebArchive {
        control,
        control_extra,
        data_files,
    })
}

pub fn write_deb(path: &Path, archive: &DebArchive) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(path)?;
    let mut ar = ar::Builder::new(file);

    ar.append(
        &ar::Header::new("debian-binary".into(), 4),
        Cursor::new(b"2.0\n"),
    )
    .map_err(|e| Error::InvalidDeb(e.to_string()))?;

    let control_tar = build_control_tar(archive)?;
    ar.append(
        &ar::Header::new("control.tar.gz".into(), control_tar.len() as u64),
        Cursor::new(control_tar),
    )
    .map_err(|e| Error::InvalidDeb(e.to_string()))?;

    let data_tar = build_data_tar(&archive.data_files)?;
    ar.append(
        &ar::Header::new("data.tar.gz".into(), data_tar.len() as u64),
        Cursor::new(data_tar),
    )
    .map_err(|e| Error::InvalidDeb(e.to_string()))?;

    Ok(())
}

pub fn extract_deb_to(path: &Path, deb_path: &Path) -> Result<()> {
    let file = File::open(deb_path)?;
    let mut archive = Archive::new(file);
    let mut data_bytes = Vec::new();

    while let Some(entry) = archive.next_entry() {
        let mut entry = entry.map_err(|e| Error::InvalidDeb(e.to_string()))?;
        let name = String::from_utf8_lossy(entry.header().identifier())
            .trim()
            .to_string();
        if name.starts_with("data.tar") {
            entry
                .read_to_end(&mut data_bytes)
                .map_err(|e| Error::InvalidDeb(e.to_string()))?;
            break;
        }
    }

    let reader = decompress(&data_bytes, detect_compression(&data_bytes))?;
    let mut tar = TarArchive::new(reader);

    for entry in tar.entries().map_err(|e| Error::InvalidDeb(e.to_string()))? {
        let mut entry = entry.map_err(|e| Error::InvalidDeb(e.to_string()))?;
        let entry_path = entry
            .path()
            .map_err(|e| Error::InvalidDeb(e.to_string()))?
            .to_path_buf();
        let mode = entry.header().mode().unwrap_or(0o644);
        let dest = path.join(entry_path.strip_prefix(".").unwrap_or(&entry_path));

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }

        match entry.header().entry_type() {
            t if t.is_dir() => {
                fs::create_dir_all(&dest)?;
            }
            t if t.is_symlink() => {
                let target = entry
                    .link_name()
                    .map_err(|e| Error::InvalidDeb(e.to_string()))?
                    .ok_or_else(|| Error::InvalidDeb("symlink without target".into()))?;
                if dest.exists() {
                    let _ = fs::remove_file(&dest);
                }
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&target, &dest).map_err(|e| {
                        Error::InvalidDeb(format!("symlink {} -> {}: {e}", dest.display(), target.display()))
                    })?;
                }
                #[cfg(not(unix))]
                {
                    return Err(Error::InvalidDeb(format!(
                        "symlink {} requires a unix target",
                        dest.display()
                    )));
                }
                apply_mode(&dest, mode)?;
            }
            tar::EntryType::Link => {
                let target = entry
                    .link_name()
                    .map_err(|e| Error::InvalidDeb(e.to_string()))?
                    .ok_or_else(|| Error::InvalidDeb("hard link without target".into()))?;
                let src = path.join(target.strip_prefix(".").unwrap_or(&target));
                if dest.exists() {
                    let _ = fs::remove_file(&dest);
                }
                fs::hard_link(&src, &dest).map_err(|e| {
                    Error::InvalidDeb(format!(
                        "hard link {} -> {}: {e}",
                        dest.display(),
                        src.display()
                    ))
                })?;
                apply_mode(&dest, mode)?;
            }
            _ => {
                let mut content = Vec::new();
                entry
                    .read_to_end(&mut content)
                    .map_err(|e| Error::InvalidDeb(e.to_string()))?;
                fs::write(&dest, &content)?;
                apply_mode(&dest, mode)?;
            }
        }
    }
    Ok(())
}

/// Remove files installed from a `.deb`. When `purge` is false, conffiles are kept.
pub fn remove_deb_from(install_root: &Path, deb_path: &Path, purge: bool) -> Result<()> {
    let entries = list_deb_data_entries(deb_path)?;
    let conffiles = if purge {
        HashSet::new()
    } else {
        conffiles_from_deb(deb_path)?
    };

    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for entry in entries {
        if entry.is_dir {
            dirs.push(entry.path);
        } else {
            files.push(entry.path);
        }
    }

    files.sort_by_key(|b| std::cmp::Reverse(b.components().count()));
    dirs.sort_by_key(|b| std::cmp::Reverse(b.components().count()));

    for rel in files {
        if !purge && conffiles.contains(&rel) {
            continue;
        }
        let full = install_root.join(&rel);
        if full.is_file() {
            let _ = fs::remove_file(&full);
        }
    }

    for rel in dirs {
        let full = install_root.join(&rel);
        if full.is_dir() {
            let _ = fs::remove_dir(&full);
        }
    }

    Ok(())
}

fn list_deb_data_entries(deb_path: &Path) -> Result<Vec<DebDataEntry>> {
    let data_bytes = read_ar_member(deb_path, |name| name.starts_with("data.tar"))?;
    let reader = decompress(&data_bytes, detect_compression(&data_bytes))?;
    let mut tar = TarArchive::new(reader);
    let mut entries = Vec::new();

    for entry in tar.entries().map_err(|e| Error::InvalidDeb(e.to_string()))? {
        let entry = entry.map_err(|e| Error::InvalidDeb(e.to_string()))?;
        let entry_path = entry
            .path()
            .map_err(|e| Error::InvalidDeb(e.to_string()))?;
        let rel = normalize_data_path(&entry_path);
        if rel.as_os_str().is_empty() {
            continue;
        }
        entries.push(DebDataEntry {
            path: rel,
            is_dir: entry.header().entry_type().is_dir(),
        });
    }

    Ok(entries)
}

fn conffiles_from_deb(deb_path: &Path) -> Result<HashSet<PathBuf>> {
    let archive = read_deb(deb_path)?;
    for (name, content) in archive.control_extra {
        if tar_entry_basename(&name) == "conffiles" {
            return Ok(parse_conffiles(&String::from_utf8_lossy(&content)));
        }
    }
    Ok(HashSet::new())
}

fn parse_conffiles(content: &str) -> HashSet<PathBuf> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| normalize_data_path(Path::new(line)))
        .collect()
}

fn read_ar_member(deb_path: &Path, matches: impl Fn(&str) -> bool) -> Result<Vec<u8>> {
    let file = File::open(deb_path)?;
    let mut archive = Archive::new(file);

    while let Some(entry) = archive.next_entry() {
        let mut entry = entry.map_err(|e| Error::InvalidDeb(e.to_string()))?;
        let name = String::from_utf8_lossy(entry.header().identifier())
            .trim()
            .to_string();
        if matches(&name) {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| Error::InvalidDeb(e.to_string()))?;
            return Ok(buf);
        }
    }

    Err(Error::InvalidDeb("missing data.tar member".into()))
}

fn normalize_data_path(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    let trimmed = raw.strip_prefix("./").unwrap_or(&raw);
    PathBuf::from(trimmed.trim_start_matches('/'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DebDataEntry {
    path: PathBuf,
    is_dir: bool,
}

#[cfg(unix)]
fn apply_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(mode);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn apply_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

pub fn build_deb_from_directory(
    output: &Path,
    control: &ControlFile,
    data_dir: &Path,
    extra_control_dir: Option<&Path>,
) -> Result<()> {
    let mut data_files = Vec::new();
    for entry in WalkDir::new(data_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if extra_control_dir.is_some_and(|d| path.starts_with(d)) {
            continue;
        }
        if path == data_dir {
            continue;
        }
        let rel = path
            .strip_prefix(data_dir)
            .map_err(|e| Error::Other(e.to_string()))?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let rel_str = format!("./{}", rel.display());

        if path.is_dir() {
            data_files.push((format!("{}/", rel_str.trim_start_matches("./")), Vec::new(), 0o755));
        } else {
            let content = std::fs::read(path)?;
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                std::fs::metadata(path)
                    .map(|m| m.permissions().mode() & 0o777)
                    .unwrap_or(0o644)
            };
            #[cfg(not(unix))]
            let mode = 0o644;
            data_files.push((rel_str, content, mode));
        }
    }

    let mut control_extra = Vec::new();
    if let Some(dir) = extra_control_dir {
        for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() && path.file_name() != Some(std::ffi::OsStr::new("control")) {
                let rel = path
                    .strip_prefix(dir)
                    .map_err(|e| Error::Other(e.to_string()))?;
                let content = std::fs::read(path)?;
                control_extra.push((rel.to_string_lossy().into_owned(), content));
            }
        }
    }

    write_deb(
        output,
        &DebArchive {
            control: control.clone(),
            control_extra,
            data_files,
        },
    )
}

fn tar_entry_basename(path: &str) -> &str {
    path.trim_start_matches("./").trim_start_matches('/')
}

fn extract_control_tar(bytes: &[u8]) -> Result<(ControlFile, Vec<(String, Vec<u8>)>)> {
    let reader = decompress(bytes, detect_compression(bytes))?;
    let mut tar = TarArchive::new(reader);
    let mut control = None;
    let mut extra = Vec::new();

    for entry in tar.entries().map_err(|e| Error::InvalidDeb(e.to_string()))? {
        let mut entry = entry.map_err(|e| Error::InvalidDeb(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| Error::InvalidDeb(e.to_string()))?
            .to_string_lossy()
            .into_owned();
        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .map_err(|e| Error::InvalidDeb(e.to_string()))?;

        if tar_entry_basename(&path) == "control" {
            control = Some(ControlFile::parse(&String::from_utf8_lossy(&content))?);
        } else if !path.ends_with('/') {
            extra.push((path, content));
        }
    }

    let control = control.ok_or_else(|| Error::InvalidDeb("missing control file".into()))?;
    Ok((control, extra))
}

fn extract_data_tar(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>, u32)>> {
    let reader = decompress(bytes, detect_compression(bytes))?;
    let mut tar = TarArchive::new(reader);
    let mut files = Vec::new();

    for entry in tar.entries().map_err(|e| Error::InvalidDeb(e.to_string()))? {
        let mut entry = entry.map_err(|e| Error::InvalidDeb(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| Error::InvalidDeb(e.to_string()))?
            .to_string_lossy()
            .into_owned();
        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .map_err(|e| Error::InvalidDeb(e.to_string()))?;
        files.push((path, content, entry.header().mode().unwrap_or(0o644)));
    }

    Ok(files)
}

fn build_control_tar(archive: &DebArchive) -> Result<Vec<u8>> {
    let mut tar_buf = Vec::new();
    {
        let enc = GzEncoder::new(&mut tar_buf, Compression::default());
        let mut builder = TarBuilder::new(enc);

        append_tar_file(&mut builder, "control", archive.control.to_string().as_bytes(), 0o644)?;
        for (name, content) in &archive.control_extra {
            append_tar_file(&mut builder, name, content, 0o644)?;
        }

        builder
            .finish()
            .map_err(|e| Error::InvalidDeb(e.to_string()))?;
    }
    Ok(tar_buf)
}

fn build_data_tar(files: &[(String, Vec<u8>, u32)]) -> Result<Vec<u8>> {
    let mut tar_buf = Vec::new();
    {
        let enc = GzEncoder::new(&mut tar_buf, Compression::default());
        let mut builder = TarBuilder::new(enc);

        for (name, content, mode) in files {
            let name = name.trim_start_matches("./");
            if name.is_empty() {
                continue;
            }
            if name.ends_with('/') {
                let mut header = Header::new_gnu();
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
                header.set_mode(*mode);
                header.set_cksum();
                builder
                    .append_data(&mut header, name, &[][..])
                    .map_err(|e| Error::InvalidDeb(e.to_string()))?;
            } else {
                append_tar_file(&mut builder, name, content, *mode)?;
            }
        }

        builder
            .finish()
            .map_err(|e| Error::InvalidDeb(e.to_string()))?;
    }
    Ok(tar_buf)
}

fn append_tar_file<W: Write>(
    builder: &mut TarBuilder<W>,
    name: &str,
    content: &[u8],
    mode: u32,
) -> Result<()> {
    let name = name.trim_start_matches("./");
    if name.is_empty() {
        return Ok(());
    }
    let mut header = Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(mode);
    header.set_cksum();
    builder
        .append_data(&mut header, name, content)
        .map_err(|e| Error::InvalidDeb(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
fn append_tar_symlink<W: Write>(
    builder: &mut TarBuilder<W>,
    name: &str,
    target: &str,
    mode: u32,
) -> Result<()> {
    let name = name.trim_start_matches("./");
    if name.is_empty() {
        return Ok(());
    }
    let mut header = Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    header.set_mode(mode);
    header.set_link_name(target)
        .map_err(|e| Error::InvalidDeb(e.to_string()))?;
    header.set_cksum();
    builder
        .append_data(&mut header, name, &[][..])
        .map_err(|e| Error::InvalidDeb(e.to_string()))?;
    Ok(())
}

fn detect_compression(bytes: &[u8]) -> CompressionKind {
    if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        CompressionKind::Gzip
    } else if bytes.len() >= 6 && &bytes[0..6] == b"\xfd7zXZ\x00" {
        CompressionKind::Xz
    } else if bytes.len() >= 4 && bytes[0] == 0x28 && bytes[1] == 0xb5 {
        CompressionKind::Zstd
    } else {
        CompressionKind::None
    }
}

enum CompressionKind {
    None,
    Gzip,
    Xz,
    Zstd,
}

fn decompress(bytes: &[u8], kind: CompressionKind) -> Result<Box<dyn Read + '_>> {
    Ok(match kind {
        CompressionKind::Gzip => Box::new(GzDecoder::new(bytes)),
        CompressionKind::Xz => Box::new(XzDecoder::new(bytes)),
        CompressionKind::Zstd => Box::new(
            ZstdDecoder::new(bytes).map_err(|e| Error::UnsupportedCompression(e.to_string()))?,
        ),
        CompressionKind::None => Box::new(Cursor::new(bytes)),
    })
}

pub fn deb_path_for(control: &ControlFile, pool_dir: &Path) -> PathBuf {
    let name = control.package.chars().next().unwrap_or('x');
    pool_dir
        .join(name.to_string())
        .join(control.full_name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_control_tar_with_dotted_path() {
        let mut tar_buf = Vec::new();
        {
            let enc = GzEncoder::new(&mut tar_buf, Compression::default());
            let mut builder = TarBuilder::new(enc);
            append_tar_file(
                &mut builder,
                "./control",
                b"Package: hello\nVersion: 1.0\n",
                0o644,
            )
            .unwrap();
            builder.finish().unwrap();
        }

        let (control, _) = extract_control_tar(&tar_buf).unwrap();
        assert_eq!(control.package, "hello");
        assert_eq!(control.version, "1.0");
    }

    #[test]
    fn round_trip_deb_archive() {
        let dir = std::env::temp_dir().join(format!("raptor-deb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let deb_path = dir.join("hello_1.0_arm64.deb");
        let control = ControlFile {
            package: "hello".into(),
            version: "1.0".into(),
            architecture: "arm64".into(),
            ..ControlFile::default()
        };
        write_deb(
            &deb_path,
            &DebArchive {
                control,
                control_extra: Vec::new(),
                data_files: vec![(
                    "./usr/bin/hello".into(),
                    b"hello".to_vec(),
                    0o755,
                )],
            },
        )
        .unwrap();

        let archive = read_deb(&deb_path).unwrap();
        assert_eq!(archive.control.package, "hello");

        let root = dir.join("rootfs");
        extract_deb_to(&root, &deb_path).unwrap();
        assert!(root.join("usr/bin/hello").is_file());

        remove_deb_from(&root, &deb_path, true).unwrap();
        assert!(!root.join("usr/bin/hello").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_deb_to_creates_symlinks() {
        let dir = std::env::temp_dir().join(format!("raptor-deb-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut data_tar = Vec::new();
        {
            let enc = GzEncoder::new(&mut data_tar, Compression::default());
            let mut builder = TarBuilder::new(enc);
            append_tar_file(
                &mut builder,
                "./usr/lib/libfoo.so.1",
                b"real-shared-object",
                0o644,
            )
            .unwrap();
            append_tar_symlink(
                &mut builder,
                "./lib/libfoo.so.1",
                "../usr/lib/libfoo.so.1",
                0o777,
            )
            .unwrap();
            builder.finish().unwrap();
        }

        let mut control_tar = Vec::new();
        {
            let enc = GzEncoder::new(&mut control_tar, Compression::default());
            let mut builder = TarBuilder::new(enc);
            append_tar_file(
                &mut builder,
                "./control",
                b"Package: libfoo\nVersion: 1.0\nArchitecture: arm64\n",
                0o644,
            )
            .unwrap();
            builder.finish().unwrap();
        }

        let deb_path = dir.join("libfoo_1.0_arm64.deb");
        {
            let file = File::create(&deb_path).unwrap();
            let mut ar = ar::Builder::new(file);
            ar.append(
                &ar::Header::new("debian-binary".into(), 4),
                Cursor::new(b"2.0\n"),
            )
            .unwrap();
            ar.append(
                &ar::Header::new("control.tar.gz".into(), control_tar.len() as u64),
                Cursor::new(&control_tar),
            )
            .unwrap();
            ar.append(
                &ar::Header::new("data.tar.gz".into(), data_tar.len() as u64),
                Cursor::new(&data_tar),
            )
            .unwrap();
        }

        let root = dir.join("rootfs");
        extract_deb_to(&root, &deb_path).unwrap();
        let link = root.join("lib/libfoo.so.1");
        assert!(link.is_symlink());
        assert_eq!(
            fs::read_link(&link).unwrap().to_string_lossy(),
            "../usr/lib/libfoo.so.1"
        );
        assert_eq!(fs::read(root.join("usr/lib/libfoo.so.1")).unwrap(), b"real-shared-object");

        let _ = fs::remove_dir_all(&dir);
    }
}
