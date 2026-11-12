use crate::xp3::{Assets, Store};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_SAMPLES: usize = 48_000 * 2 * 600;
const MAX_AUDIO_BYTES: usize = 64 * 1024 * 1024;
const MAX_AUDIO_SECONDS: u64 = 600;
const MAX_VIDEO_BYTES: usize = 512 * 1024 * 1024;
const MAX_VIDEO_SECONDS: u64 = 6 * 60 * 60;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Export only fully verified XP3 bytes into an app-owned cache directory.
/// Reuse is content-addressed, but never bypasses the source archive checksum.
pub fn export_video(assets: &Store, name: &str, directory: &Path) -> Result<PathBuf, String> {
    let name = assets.find(name, &["mp4"]).map_err(|e| e.to_string())?;
    if !name.to_ascii_lowercase().ends_with(".mp4") {
        return Err("video must be an MP4 asset".into());
    }
    let bytes = assets.bytes(&name).map_err(|e| e.to_string())?;
    validate_mp4(&bytes)?;
    let digest = Sha256::digest(&bytes);
    let filename = format!("koikari-video-v1-{digest:x}.mp4");
    if let Ok(metadata) = fs::symlink_metadata(directory) {
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("video cache must be a real directory".into());
        }
    }
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let directory = directory.canonicalize().map_err(|e| e.to_string())?;
    let destination = directory.join(filename);
    if cached_video_matches(&destination, bytes.len() as u64, digest.as_slice())? {
        return Ok(destination);
    }
    let (partial, mut file) = create_partial(&directory)?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    // Recheck the leaf before replacing it; cache directories are app-owned.
    cached_video_matches(&destination, bytes.len() as u64, digest.as_slice())?;
    fs::rename(&partial.0, &destination).map_err(|e| e.to_string())?;
    File::open(&directory)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(destination)
}

/// Call only after stopping players and joining all export workers.
/// Removes this module's regular cache/partial files, never directories, links or other files.
pub fn clear_video_cache(directory: &Path) -> Result<(), String> {
    clean_video_cache(directory, true)
}

/// At startup, before launching export workers, remove abandoned partials but retain reusable MP4s.
pub fn prune_video_partials(directory: &Path) -> Result<(), String> {
    clean_video_cache(directory, false)
}

fn clean_video_cache(directory: &Path, complete: bool) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("video cache must be a real directory".into());
    }
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let cached = name
            .strip_prefix("koikari-video-v1-")
            .and_then(|n| n.strip_suffix(".mp4"))
            .is_some_and(|hash| {
                hash.len() == 64
                    && hash
                        .bytes()
                        .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
            });
        let partial = name
            .strip_prefix(".koikari-video-")
            .and_then(|n| n.strip_suffix(".part"))
            .is_some_and(|parts| {
                let fields: Vec<_> = parts.split('-').collect();
                fields.len() == 2
                    && fields
                        .iter()
                        .all(|field| !field.is_empty() && field.bytes().all(|v| v.is_ascii_digit()))
            });
        if ((complete && cached) || partial)
            && entry.file_type().map_err(|e| e.to_string())?.is_file()
        {
            fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn cached_video_matches(path: &Path, size: u64, digest: &[u8]) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("unsafe video cache entry".into());
    }
    if metadata.len() != size {
        return Ok(false);
    }
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut total = 0u64;
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > size {
            return Ok(false);
        }
        hash.update(&buffer[..count]);
    }
    Ok(total == size && hash.finalize().as_slice() == digest)
}

struct PartialVideo(PathBuf);
impl Drop for PartialVideo {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn create_partial(directory: &Path) -> Result<(PartialVideo, File), String> {
    for _ in 0..128 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            ".koikari-video-{}-{sequence}.part",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => return Ok((PartialVideo(path), file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("unable to reserve a video cache temporary file".into())
}

type Mp4Box<'a> = (&'a [u8], &'a [u8], &'a [u8]);

fn mp4_box(bytes: &[u8]) -> Result<Mp4Box<'_>, String> {
    if bytes.len() < 8 {
        return Err("truncated MP4 box".into());
    }
    let short = u32::from_be_bytes(bytes[..4].try_into().unwrap());
    let (size, header) = match short {
        0 => (bytes.len() as u64, 8),
        1 if bytes.len() >= 16 => (u64::from_be_bytes(bytes[8..16].try_into().unwrap()), 16),
        1 => return Err("truncated extended MP4 box".into()),
        value => (value as u64, 8),
    };
    let size = usize::try_from(size).map_err(|_| "MP4 box size overflow")?;
    if size < header || size > bytes.len() {
        return Err("invalid MP4 box size".into());
    }
    Ok((&bytes[4..8], &bytes[header..size], &bytes[size..]))
}

fn movie_duration(bytes: &[u8]) -> Result<(), String> {
    let version = *bytes.first().ok_or("missing MP4 movie header")?;
    let (rate_at, duration_at, duration_size, minimum) = match version {
        0 => (12, 16, 4, 100),
        1 => (20, 24, 8, 112),
        _ => return Err("unsupported MP4 movie header version".into()),
    };
    if bytes.len() < minimum {
        return Err("truncated MP4 movie header".into());
    }
    let rate = u32::from_be_bytes(bytes[rate_at..rate_at + 4].try_into().unwrap()) as u64;
    let duration = if duration_size == 4 {
        u32::from_be_bytes(bytes[duration_at..duration_at + 4].try_into().unwrap()) as u64
    } else {
        u64::from_be_bytes(bytes[duration_at..duration_at + 8].try_into().unwrap())
    };
    if rate == 0 || duration == 0 || duration > rate * MAX_VIDEO_SECONDS {
        return Err("invalid or excessive MP4 duration".into());
    }
    Ok(())
}

fn validate_mp4(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_VIDEO_BYTES {
        return Err("video exceeds 512 MiB export budget".into());
    }
    let mut remaining = bytes;
    let (mut ftyp, mut moov, mut mdat) = (false, false, false);
    let mut boxes = 0;
    while !remaining.is_empty() {
        boxes += 1;
        if boxes > 65_536 {
            return Err("MP4 box count exceeds budget".into());
        }
        let (tag, body, rest) = mp4_box(remaining)?;
        match tag {
            b"ftyp" => {
                if ftyp || body.len() < 8 || body.len() % 4 != 0 {
                    return Err("invalid MP4 file type box".into());
                }
                let supported = |brand: &[u8]| {
                    matches!(
                        brand,
                        b"isom"
                            | b"iso2"
                            | b"iso4"
                            | b"iso5"
                            | b"iso6"
                            | b"mp41"
                            | b"mp42"
                            | b"avc1"
                            | b"M4V "
                    )
                };
                if !supported(&body[..4])
                    && !body[8..].as_chunks::<4>().0.iter().any(|v| supported(v))
                {
                    return Err("unsupported MP4 file type".into());
                }
                ftyp = true;
            }
            b"moov" => {
                if moov {
                    return Err("duplicate MP4 movie box".into());
                }
                let mut children = body;
                let mut header = false;
                while !children.is_empty() {
                    boxes += 1;
                    if boxes > 65_536 {
                        return Err("MP4 box count exceeds budget".into());
                    }
                    let (tag, body, rest) = mp4_box(children)?;
                    if tag == b"mvhd" {
                        if header {
                            return Err("duplicate MP4 movie header".into());
                        }
                        movie_duration(body)?;
                        header = true;
                    }
                    children = rest;
                }
                if !header {
                    return Err("missing MP4 movie header".into());
                }
                moov = true;
            }
            b"mdat" => mdat |= !body.is_empty(),
            _ => (),
        }
        remaining = rest;
    }
    if !ftyp || !moov || !mdat {
        return Err("incomplete MP4 container".into());
    }
    Ok(())
}

fn audio_limit(rate: u32, channels: u16) -> Result<usize, String> {
    if !(1..=2).contains(&channels) || rate == 0 || rate > 192_000 {
        return Err("unsupported audio format".into());
    }
    Ok(((rate as u64 * channels as u64 * MAX_AUDIO_SECONDS) as usize).min(MAX_SAMPLES))
}

fn validate_ogg(bytes: &[u8]) -> Result<u64, String> {
    let mut remaining = bytes;
    let mut serial = None;
    let mut sequence = 0u32;
    let mut final_granule = None;
    while !remaining.is_empty() {
        if remaining.len() < 27 || &remaining[..4] != b"OggS" || remaining[4] != 0 {
            return Err("invalid or truncated Ogg page".into());
        }
        let flags = remaining[5];
        let page_serial = u32::from_le_bytes(remaining[14..18].try_into().unwrap());
        let page_sequence = u32::from_le_bytes(remaining[18..22].try_into().unwrap());
        if flags & !7 != 0 || final_granule.is_some() {
            return Err("invalid Ogg flags or data after EOS".into());
        }
        if let Some(serial) = serial {
            if serial != page_serial || flags & 2 != 0 || sequence != page_sequence {
                return Err("chained, multiplexed or discontinuous Ogg stream".into());
            }
        } else if flags & 2 == 0 || flags & 1 != 0 || page_sequence != 0 {
            return Err("missing Ogg beginning of stream".into());
        }
        serial = Some(page_serial);
        sequence = page_sequence.wrapping_add(1);
        let table_end = 27 + remaining[26] as usize;
        let table = remaining
            .get(27..table_end)
            .ok_or("truncated Ogg segment table")?;
        let size = table_end + table.iter().map(|v| *v as usize).sum::<usize>();
        if size > remaining.len() {
            return Err("truncated Ogg page body".into());
        }
        if flags & 4 != 0 {
            if table.is_empty() || table.last() == Some(&255) {
                return Err("incomplete Ogg final packet".into());
            }
            final_granule = Some(u64::from_le_bytes(remaining[6..14].try_into().unwrap()));
        }
        remaining = &remaining[size..];
    }
    let granule = final_granule.ok_or("truncated Ogg stream: EOS missing")?;
    if granule == u64::MAX {
        return Err("invalid Ogg final granule".into());
    }
    // Exhaust the CRC-checking reader before either codec can accept the stream.
    let mut reader = ogg::reading::PacketReader::new(Cursor::new(bytes));
    let mut packets = 0;
    while let Some(packet) = reader.read_packet().map_err(|e| e.to_string())? {
        packets += 1;
        if packets > 1_000_000 || packet.data.is_empty() {
            return Err("invalid Ogg packet or packet budget exceeded".into());
        }
    }
    Ok(granule)
}

fn opus_tags(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 16 || !bytes.starts_with(b"OpusTags") {
        return Err("invalid OpusTags".into());
    }
    let vendor = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let mut cursor = 12usize.checked_add(vendor).ok_or("OpusTags overflow")?;
    let take = |cursor: &mut usize| -> Result<usize, String> {
        let end = cursor.checked_add(4).ok_or("OpusTags overflow")?;
        let word = bytes.get(*cursor..end).ok_or("truncated OpusTags")?;
        *cursor = end;
        Ok(u32::from_le_bytes(word.try_into().unwrap()) as usize)
    };
    let comments = take(&mut cursor)?;
    if comments > bytes.len() / 4 {
        return Err("invalid OpusTags comment count".into());
    }
    for _ in 0..comments {
        let size = take(&mut cursor)?;
        cursor = cursor.checked_add(size).ok_or("OpusTags overflow")?;
        if cursor > bytes.len() {
            return Err("truncated OpusTags comment".into());
        }
    }
    Ok(())
}

pub fn wav(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > MAX_AUDIO_BYTES {
        return Err("audio exceeds 64 MiB input budget".into());
    }
    let granule = validate_ogg(bytes)?;
    let mut ogg = ogg::reading::PacketReader::new(Cursor::new(bytes));
    let first = ogg
        .read_packet()
        .map_err(|e| e.to_string())?
        .ok_or("empty Ogg stream")?;
    let (samples, rate, channels) = if first.data.starts_with(b"OpusHead") {
        if first.data.len() < 19 || !(1..=15).contains(&first.data[8]) || first.data[18] != 0 {
            return Err("unsupported Opus header/mapping".into());
        }
        let channels = first.data[9] as usize;
        let kind = match channels {
            1 => opus::Channels::Mono,
            2 => opus::Channels::Stereo,
            _ => return Err("unsupported Opus channels".into()),
        };
        let preskip = u16::from_le_bytes([first.data[10], first.data[11]]) as usize;
        let gain = i16::from_le_bytes([first.data[16], first.data[17]]) as f32;
        let gain = 10f32.powf(gain / (256.0 * 20.0));
        let tags = ogg
            .read_packet()
            .map_err(|e| e.to_string())?
            .ok_or("missing OpusTags")?;
        opus_tags(&tags.data)?;
        let limit = audio_limit(48_000, channels as u16)?;
        if granule > limit as u64 / channels as u64 + preskip as u64 {
            return Err("audio exceeds ten-minute duration budget".into());
        }
        let mut decoder = opus::Decoder::new(48000, kind).map_err(|e| e.to_string())?;
        let mut output = Vec::new();
        let mut buffer = vec![0i16; 5760 * channels];
        let mut final_granule = None;
        let mut last_count = 0;
        while let Some(packet) = ogg.read_packet().map_err(|e| e.to_string())? {
            let count = decoder
                .decode(&packet.data, &mut buffer, false)
                .map_err(|e| e.to_string())?
                * channels;
            if output.len() + count > limit + (preskip + 5760) * channels {
                return Err("audio exceeds ten-minute decode budget".into());
            }
            output.extend(buffer[..count].iter().map(|v| {
                (*v as f32 * gain)
                    .round()
                    .clamp(i16::MIN as f32, i16::MAX as f32) as i16
            }));
            last_count = count;
            if packet.last_in_stream() {
                final_granule = Some(
                    usize::try_from(packet.absgp_page()).map_err(|_| "Opus granule overflow")?,
                );
            }
        }
        let end = final_granule
            .ok_or("truncated Opus stream: EOS missing")?
            .checked_mul(channels)
            .ok_or("Opus length overflow")?;
        let start = preskip
            .checked_mul(channels)
            .ok_or("Opus preskip overflow")?;
        if start >= end || end > output.len() || end < output.len() - last_count {
            return Err("invalid Opus preskip or final granule".into());
        }
        (output[start..end].to_vec(), 48000u32, channels as u16)
    } else if first.data.starts_with(b"\x01vorbis") {
        if first.data.len() < 30 {
            return Err("truncated Vorbis identification header".into());
        }
        audio_limit(
            u32::from_le_bytes(first.data[12..16].try_into().unwrap()),
            first.data[11] as u16,
        )?;
        let mut reader = lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes))
            .map_err(|e| e.to_string())?;
        let channels = reader.ident_hdr.audio_channels as u16;
        let rate = reader.ident_hdr.audio_sample_rate;
        let limit = audio_limit(rate, channels)?;
        if granule > limit as u64 / channels as u64 {
            return Err("audio exceeds ten-minute duration budget".into());
        }
        let mut samples = Vec::new();
        let mut last_count = 0;
        while let Some(packet) = reader.read_dec_packet_itl().map_err(|e| e.to_string())? {
            if samples.len() + packet.len() > limit + 8192 * channels as usize {
                return Err("audio exceeds decode budget".into());
            }
            last_count = packet.len();
            samples.extend(packet);
        }
        let end = usize::try_from(granule)
            .map_err(|_| "Vorbis granule overflow")?
            .checked_mul(channels as usize)
            .ok_or("Vorbis length overflow")?;
        if end == 0 || end > samples.len() || end < samples.len() - last_count {
            return Err("invalid Vorbis final granule".into());
        }
        // Lewton cannot trim EOS when all audio packets fit on the first audio page.
        samples.truncate(end);
        (samples, rate, channels)
    } else {
        return Err("not Opus or Vorbis".into());
    };
    pcm_wav(&samples, rate, channels)
}

fn pcm_wav(samples: &[i16], rate: u32, channels: u16) -> Result<Vec<u8>, String> {
    let limit = audio_limit(rate, channels)?;
    if samples.is_empty()
        || !samples.len().is_multiple_of(channels as usize)
        || samples.len() > limit
    {
        return Err("invalid or excessive PCM duration".into());
    }
    let size = u32::try_from(samples.len().checked_mul(2).ok_or("WAV overflow")?)
        .map_err(|_| "WAV too large")?;
    let mut out = Vec::with_capacity(size as usize + 44);
    out.extend(b"RIFF");
    out.extend((size + 36).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(channels.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * channels as u32 * 2).to_le_bytes());
    out.extend((channels * 2).to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(size.to_le_bytes());
    for sample in samples {
        out.extend(sample.to_le_bytes());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aborted_export_removes_owned_partial_file() {
        let directory = tempfile::tempdir().unwrap();
        let partial_path;
        {
            let (partial, mut file) = create_partial(directory.path()).unwrap();
            file.write_all(b"unfinished").unwrap();
            partial_path = partial.0.clone();
            assert!(partial_path.exists());
        }
        assert!(!partial_path.exists());
    }

    #[test]
    fn duration_limits_use_rate_and_channels() {
        assert_eq!(audio_limit(8000, 1).unwrap(), 8000 * 600);
        assert_eq!(audio_limit(48000, 2).unwrap(), MAX_SAMPLES);
        assert!(audio_limit(0, 1).is_err());
        assert!(audio_limit(192001, 1).is_err());
        assert!(audio_limit(48000, 3).is_err());
        assert!(pcm_wav(&[1], 48000, 2).is_err());
        assert!(pcm_wav(&[], 48000, 1).is_err());
    }
    #[test]
    fn rejects_unknown_and_truncated() {
        assert!(wav(b"not media").is_err());
        assert!(wav(b"OggS").is_err());
    }
    #[test]
    fn wave_header_matches_pcm() {
        let w = pcm_wav(&[1, -1], 48000, 1).unwrap();
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(w.len(), 48);
        assert_eq!(&w[44..], &[1, 0, 255, 255]);
    }
}
