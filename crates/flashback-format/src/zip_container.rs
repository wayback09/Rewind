use crate::error::{FormatError, Result};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::Path;
use zip::ZipArchive;

#[derive(Debug)]
pub struct ZipEntryInfo {
    pub name: String,
    pub is_dir: bool,
    pub file_size: u64,
    pub compressed_size: u64,
    pub compression_method: String,
}

pub fn open_zip_readonly<P: AsRef<Path>>(path: P) -> Result<ZipArchive<File>> {
    let p = path.as_ref();
    let file = File::open(p)
        .map_err(|e| FormatError::new(format!("failed to open ZIP '{}': {}", p.display(), e)))?;
    ZipArchive::new(file)
        .map_err(|e| FormatError::new(format!("failed to parse ZIP '{}': {}", p.display(), e)))
}

pub fn list_entries<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Vec<ZipEntryInfo> {
    let mut out = Vec::new();
    for i in 0..archive.len() {
        if let Ok(f) = archive.by_index(i) {
            out.push(ZipEntryInfo {
                name: f.name().to_string(),
                is_dir: f.is_dir(),
                file_size: f.size(),
                compressed_size: f.compressed_size(),
                compression_method: format!("{:?}", f.compression()),
            });
        }
    }
    out
}

pub fn read_entry_bytes<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>> {
    let mut file = archive
        .by_name(name)
        .map_err(|e| FormatError::new(format!("ZIP entry '{}' not found: {}", name, e)))?;
    let mut buf = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut buf).map_err(|e| {
        FormatError::new(format!("failed to read ZIP entry '{}': {}", name, e))
            .with_context(format!("size {}", file.size()))
    })?;
    Ok(buf)
}

pub fn has_entry<R: Read + Seek>(archive: &mut ZipArchive<R>, name: &str) -> bool {
    archive.by_name(name).is_ok()
}

pub fn find_chunk_names<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Vec<String> {
    let mut names = Vec::new();
    for i in 0..archive.len() {
        if let Ok(f) = archive.by_index(i) {
            let n = f.name();
            if n.starts_with('c') && n.ends_with(".flashback") && !f.is_dir() {
                names.push(n.to_string());
            }
        }
    }
    names.sort();
    names
}

pub fn find_cache_shards<R: Read + Seek>(archive: &mut ZipArchive<R>) -> BTreeMap<u32, String> {
    let mut map = BTreeMap::new();
    for i in 0..archive.len() {
        if let Ok(f) = archive.by_index(i) {
            let n = f.name();
            if n.starts_with("level_chunk_caches/") && !f.is_dir() {
                if let Some(suffix) = n.strip_prefix("level_chunk_caches/") {
                    if let Ok(idx) = suffix.parse::<u32>() {
                        map.insert(idx, n.to_string());
                    }
                }
            }
        }
    }
    map
}
