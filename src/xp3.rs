use flate2::read::ZlibDecoder;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use thiserror::Error;

const MAGIC: &[u8] = b"XP3\r\n \n\x1a\x8bg\x01";
const INDEX_LIMIT: u64 = 64 * 1024 * 1024;
const ASSET_LIMIT: u64 = 512 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid XP3: {0}")]
    Invalid(String),
    #[error("Asset not found: {0}")]
    Missing(String),
    #[error("Ambiguous asset name: {0}")]
    Ambiguous(String),
    #[error("Protected or unsupported XP3 entry: {0}")]
    Protected(String),
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug)]
struct Segment {
    flags: u32,
    offset: u64,
    original: u64,
    stored: u64,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    flags: u32,
    checksum: u32,
    segments: Vec<Segment>,
}

pub struct Archive {
    path: PathBuf,
    length: u64,
    pub entries: BTreeMap<String, Entry>,
    digest: Vec<u8>,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
fn number(bytes: &[u8], offset: usize, count: usize) -> Result<u64> {
    let end = offset
        .checked_add(count)
        .ok_or_else(|| invalid("offset overflow"))?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| invalid("truncated integer"))?;
    Ok(value
        .iter()
        .enumerate()
        .fold(0, |n, (i, b)| n | ((*b as u64) << (i * 8))))
}
fn u64_at(bytes: &[u8], offset: usize) -> Result<u64> {
    number(bytes, offset, 8)
}
fn chunks(mut bytes: &[u8]) -> Result<Vec<(&[u8], &[u8])>> {
    let mut result = Vec::new();
    while !bytes.is_empty() {
        let tag = bytes.get(..4).ok_or_else(|| invalid("truncated chunk"))?;
        let length = usize::try_from(u64_at(bytes, 4)?).map_err(|_| invalid("chunk too large"))?;
        let end = 12usize
            .checked_add(length)
            .ok_or_else(|| invalid("chunk overflow"))?;
        let body = bytes
            .get(12..end)
            .ok_or_else(|| invalid("chunk extends beyond index"))?;
        result.push((tag, body));
        bytes = &bytes[end..];
    }
    Ok(result)
}
fn read_range(file: &mut File, length: u64, offset: u64, size: u64, limit: u64) -> Result<Vec<u8>> {
    if size > limit
        || offset
            .checked_add(size)
            .filter(|end| *end <= length)
            .is_none()
    {
        return Err(invalid("out-of-bounds or oversized read"));
    }
    let mut bytes = vec![0; size as usize];
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}
fn inflate(bytes: &[u8], expected: u64, limit: u64) -> Result<Vec<u8>> {
    if expected > limit {
        return Err(invalid("decompressed data exceeds limit"));
    }
    let mut output = Vec::new();
    ZlibDecoder::new(bytes)
        .take(expected + 1)
        .read_to_end(&mut output)?;
    if output.len() as u64 != expected {
        return Err(invalid("decompressed size mismatch"));
    }
    Ok(output)
}
pub fn normalize(name: &str) -> Result<String> {
    let name = name.replace('\\', "/").to_lowercase();
    if name.is_empty()
        || name.chars().any(char::is_control)
        || name.starts_with('/')
        || name.contains(':')
        || name
            .split('/')
            .any(|p| p.is_empty() || p == ".." || p == ".")
    {
        return Err(invalid("unsafe or empty asset path"));
    }
    Ok(name)
}

impl Archive {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path)?;
        let length = file.metadata()?.len();
        let header = read_range(&mut file, length, 0, 19, 19)?;
        if &header[..11] != MAGIC {
            return Err(invalid("bad signature"));
        }
        let mut offset = u64_at(&header, 11)?;
        let mut seen = HashSet::new();
        let mut entries = BTreeMap::new();
        let mut digest = Sha256::new();
        digest.update(length.to_le_bytes());
        loop {
            if !seen.insert(offset) || seen.len() > 64 {
                return Err(invalid("cyclic/excessive index chain"));
            }
            let flags = read_range(&mut file, length, offset, 1, 1)?[0];
            if flags & !0x81 != 0 {
                return Err(invalid("unsupported index flags"));
            }
            let (data, next) = match flags & 7 {
                0 => {
                    let bytes = read_range(&mut file, length, offset + 1, 8, 8)?;
                    let size = u64_at(&bytes, 0)?;
                    (
                        read_range(&mut file, length, offset + 9, size, INDEX_LIMIT)?,
                        offset.checked_add(9).and_then(|n| n.checked_add(size)),
                    )
                }
                1 => {
                    let bytes = read_range(&mut file, length, offset + 1, 16, 16)?;
                    let packed = u64_at(&bytes, 0)?;
                    let original = u64_at(&bytes, 8)?;
                    let data = read_range(&mut file, length, offset + 17, packed, INDEX_LIMIT)?;
                    (
                        inflate(&data, original, INDEX_LIMIT)?,
                        offset.checked_add(17).and_then(|n| n.checked_add(packed)),
                    )
                }
                _ => return Err(invalid("unsupported index compression")),
            };
            digest.update(&data);
            for (tag, chunk) in chunks(&data)? {
                if tag != b"File" {
                    continue;
                }
                let mut entry = Entry {
                    name: String::new(),
                    size: 0,
                    flags: 0,
                    checksum: 0,
                    segments: Vec::new(),
                };
                let (mut has_info, mut has_hash, mut has_segments) = (false, false, false);
                let mut archived_size = 0;
                for (kind, body) in chunks(chunk)? {
                    match kind {
                        b"info" => {
                            if has_info {
                                return Err(invalid("duplicate info chunk"));
                            }
                            entry.flags = number(body, 0, 4)? as u32;
                            entry.size = u64_at(body, 4)?;
                            archived_size = u64_at(body, 12)?;
                            let chars = number(body, 20, 2)? as usize;
                            let text = body
                                .get(22..22 + chars * 2)
                                .ok_or_else(|| invalid("truncated filename"))?;
                            let words: Vec<u16> = text
                                .as_chunks::<2>()
                                .0
                                .iter()
                                .map(|v| u16::from_le_bytes([v[0], v[1]]))
                                .collect();
                            entry.name = String::from_utf16(&words)
                                .map_err(|_| invalid("bad UTF-16 filename"))?;
                            has_info = true;
                        }
                        b"segm" => {
                            if has_segments || body.len() % 28 != 0 {
                                return Err(invalid("invalid segment table"));
                            }
                            has_segments = true;
                            for part in body.as_chunks::<28>().0 {
                                entry.segments.push(Segment {
                                    flags: number(part, 0, 4)? as u32,
                                    offset: u64_at(part, 4)?,
                                    original: u64_at(part, 12)?,
                                    stored: u64_at(part, 20)?,
                                });
                            }
                        }
                        b"adlr" => {
                            if has_hash {
                                return Err(invalid("duplicate checksum chunk"));
                            }
                            entry.checksum = number(body, 0, 4)? as u32;
                            has_hash = true;
                        }
                        _ => (),
                    }
                }
                if !has_info || !has_hash || !has_segments {
                    return Err(invalid("incomplete file entry"));
                }
                let total = entry.segments.iter().try_fold(0u64, |n, s| {
                    n.checked_add(s.original)
                        .ok_or_else(|| invalid("segment size overflow"))
                })?;
                if total != entry.size {
                    return Err(invalid("segment table size mismatch"));
                }
                let stored_total = entry.segments.iter().try_fold(0u64, |n, s| {
                    n.checked_add(s.stored)
                        .ok_or_else(|| invalid("stored size overflow"))
                })?;
                if stored_total != archived_size {
                    return Err(invalid("stored size mismatch"));
                }
                for segment in &entry.segments {
                    if segment
                        .offset
                        .checked_add(segment.stored)
                        .filter(|end| *end <= length)
                        .is_none()
                    {
                        return Err(invalid("segment outside archive"));
                    }
                }
                let key = normalize(&entry.name)?;
                if entries.insert(key, entry).is_some() {
                    return Err(invalid("duplicate normalized archive path"));
                }
            }
            if flags & 0x80 == 0 {
                break;
            }
            let next = next.ok_or_else(|| invalid("index offset overflow"))?;
            offset = u64_at(&read_range(&mut file, length, next, 8, 8)?, 0)?;
        }
        Ok(Self {
            path,
            length,
            entries,
            digest: digest.finalize().to_vec(),
        })
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>> {
        if entry.flags != 0 {
            return Err(Error::Protected(entry.name.clone()));
        }
        if entry.size > ASSET_LIMIT {
            return Err(invalid("asset exceeds memory budget"));
        }
        let mut file = File::open(&self.path)?;
        let mut output = Vec::with_capacity(entry.size as usize);
        for segment in &entry.segments {
            let bytes = read_range(
                &mut file,
                self.length,
                segment.offset,
                segment.stored,
                ASSET_LIMIT,
            )?;
            let bytes = match segment.flags {
                0 if segment.stored == segment.original => bytes,
                1 => inflate(&bytes, segment.original, ASSET_LIMIT)
                    .map_err(|e| invalid(format!("{} at {}: {e}", entry.name, segment.offset)))?,
                _ => return Err(invalid("unsupported segment flags/size")),
            };
            output.extend(bytes);
        }
        if output.len() as u64 != entry.size || adler32(&output) != entry.checksum {
            return Err(invalid(format!("checksum/size mismatch: {}", entry.name)));
        }
        Ok(output)
    }
}

pub fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for group in bytes.chunks(5552) {
        for byte in group {
            a += *byte as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

pub trait Assets {
    fn bytes(&self, name: &str) -> Result<Vec<u8>>;
}

impl Assets for Archive {
    fn bytes(&self, name: &str) -> Result<Vec<u8>> {
        let key = normalize(name)?;
        self.read(
            self.entries
                .get(&key)
                .ok_or_else(|| Error::Missing(name.to_owned()))?,
        )
    }
}

pub struct Store {
    archives: Vec<Archive>,
    pub fingerprint: String,
}
impl Store {
    pub fn open(directory: impl AsRef<Path>) -> Result<Self> {
        let directory = directory.as_ref();
        let names = ["data.xp3", "adult.xp3", "patch.xp3"]
            .into_iter()
            .map(str::to_owned)
            .chain((1..=50).map(|number| format!("patch{number}.xp3")));
        let mut archives = Vec::new();
        for (index, name) in names.enumerate() {
            let path = directory.join(name);
            if index == 0 || path.try_exists()? {
                archives.push(Archive::open(path)?);
            }
        }
        let mut digest = Sha256::new();
        for archive in &archives {
            digest.update(&archive.digest);
        }
        Ok(Self {
            archives,
            fingerprint: format!("{:x}", digest.finalize()),
        })
    }
    pub fn archive_names(&self) -> Vec<String> {
        self.archives
            .iter()
            .filter_map(|archive| archive.path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect()
    }
    pub fn names(&self) -> Vec<String> {
        let mut names = BTreeMap::new();
        for archive in &self.archives {
            for (key, value) in &archive.entries {
                names.insert(key.clone(), value.name.clone());
            }
        }
        names.into_values().collect()
    }
    fn resolve(&self, name: &str) -> Result<(&Archive, &Entry)> {
        let name = normalize(name)?;
        for archive in self.archives.iter().rev() {
            if let Some(entry) = archive.entries.get(&name) {
                return Ok((archive, entry));
            }
            let mut candidates: Vec<_> = archive
                .entries
                .iter()
                .filter(|(key, _)| key.rsplit('/').next() == Some(name.as_str()))
                .map(|(_, e)| e)
                .collect();
            if candidates.len() > 1
                && candidates.iter().any(|entry| {
                    entry.checksum != candidates[0].checksum || entry.size != candidates[0].size
                })
            {
                return Err(Error::Ambiguous(name));
            }
            candidates.sort_by_key(|entry| entry.name.len());
            if let Some(entry) = candidates.first() {
                return Ok((archive, entry));
            }
        }
        Err(Error::Missing(name))
    }
    pub fn find(&self, name: &str, extensions: &[&str]) -> Result<String> {
        match self.resolve(name) {
            Ok((_, entry)) => return Ok(entry.name.clone()),
            Err(Error::Missing(_)) => (),
            Err(error) => return Err(error),
        }
        for extension in extensions {
            match self.resolve(&format!("{name}.{extension}")) {
                Ok((_, entry)) => return Ok(entry.name.clone()),
                Err(Error::Missing(_)) => (),
                Err(error) => return Err(error),
            }
        }
        Err(Error::Missing(name.to_owned()))
    }
    /// Reject oversized decompressed entries before reading or allocating their contents.
    pub fn bytes_limited(&self, name: &str, limit: u64) -> Result<Vec<u8>> {
        let (archive, entry) = self.resolve(name)?;
        if entry.size > limit {
            return Err(invalid("asset exceeds requested byte limit"));
        }
        archive.read(entry)
    }
    pub fn text(&self, name: &str) -> Result<String> {
        let bytes = self.bytes(name)?;
        if bytes.starts_with(&[0xff, 0xfe]) {
            if bytes.len() % 2 != 0 {
                return Err(invalid("odd UTF-16 script"));
            }
            let words: Vec<_> = bytes[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|v| u16::from_le_bytes([v[0], v[1]]))
                .collect();
            String::from_utf16(&words).map_err(|_| invalid("invalid UTF-16 script"))
        } else {
            String::from_utf8(
                bytes
                    .strip_prefix(&[0xef, 0xbb, 0xbf])
                    .unwrap_or(&bytes)
                    .to_vec(),
            )
            .map_err(|_| invalid("unsupported script encoding"))
        }
    }
}
impl Assets for Store {
    fn bytes(&self, name: &str) -> Result<Vec<u8>> {
        let (archive, entry) = self.resolve(name)?;
        archive.read(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_are_safe() {
        for path in ["../a", "/a", "a/../b", "C:\\a", "a//b"] {
            assert!(normalize(path).is_err());
        }
        assert_eq!(normalize("A\\B.KS").unwrap(), "a/b.ks");
    }
    #[test]
    fn malformed_chunks_fail() {
        assert!(chunks(b"File").is_err());
        let mut b = b"File".to_vec();
        b.extend(u64::MAX.to_le_bytes());
        assert!(chunks(&b).is_err());
    }
    #[test]
    fn known_adler() {
        assert_eq!(adler32(b"Wikipedia"), 0x11e60398);
    }
    #[test]
    fn wrong_signature_fails() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bad");
        std::fs::write(&p, [0; 19]).unwrap();
        assert!(Archive::open(p).is_err());
    }
}
