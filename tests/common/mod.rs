use flate2::{write::ZlibEncoder, Compression};
use koikari_reborn::xp3::adler32;
use std::io::Write;

fn chunk(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = tag.to_vec();
    bytes.extend((body.len() as u64).to_le_bytes());
    bytes.extend(body);
    bytes
}
fn compress(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

pub fn archive(
    files: &[(&str, &[u8])],
    packed_index: bool,
    packed_segments: bool,
    checksum_delta: u32,
    protected: bool,
) -> Vec<u8> {
    let mut out = b"XP3\r\n \n\x1a\x8bg\x01".to_vec();
    out.extend(0u64.to_le_bytes());
    let mut index = Vec::new();
    for (name, data) in files {
        let mut segments = Vec::new();
        let mut stored = 0u64;
        for piece in data.chunks((data.len() / 2).max(1)) {
            let bytes = if packed_segments {
                compress(piece)
            } else {
                piece.to_vec()
            };
            segments.extend((u32::from(packed_segments)).to_le_bytes());
            segments.extend((out.len() as u64).to_le_bytes());
            segments.extend((piece.len() as u64).to_le_bytes());
            segments.extend((bytes.len() as u64).to_le_bytes());
            stored += bytes.len() as u64;
            out.extend(bytes);
        }
        let mut info = u32::from(protected).to_le_bytes().to_vec();
        info.extend((data.len() as u64).to_le_bytes());
        info.extend(stored.to_le_bytes());
        let name: Vec<u16> = name.encode_utf16().collect();
        info.extend((name.len() as u16).to_le_bytes());
        for c in name {
            info.extend(c.to_le_bytes());
        }
        let mut file = chunk(b"info", &info);
        file.extend(chunk(b"segm", &segments));
        file.extend(chunk(
            b"adlr",
            &adler32(data).wrapping_add(checksum_delta).to_le_bytes(),
        ));
        index.extend(chunk(b"File", &file));
    }
    let offset = out.len() as u64;
    out[11..19].copy_from_slice(&offset.to_le_bytes());
    if packed_index {
        let packed = compress(&index);
        out.push(1);
        out.extend((packed.len() as u64).to_le_bytes());
        out.extend((index.len() as u64).to_le_bytes());
        out.extend(packed);
    } else {
        out.push(0);
        out.extend((index.len() as u64).to_le_bytes());
        out.extend(index);
    }
    out
}

pub fn game(script: &str) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let data = archive(
        &[
            ("ui/title/TI_base_0.webp", b"synthetic title"),
            ("Init_VariableSTAND.ks", b"; synthetic metadata"),
            (
                "Init_VariableADV.ks",
                b"tf.adv_NameMappings = %[\n\"Actor\" => \"Character\"\n];",
            ),
            ("DB_EV.ks", b"lf.EV_EventListInfo = [\"intro\"]"),
            ("intro.ks", script.as_bytes()),
        ],
        true,
        true,
        0,
        false,
    );
    std::fs::write(directory.path().join("data.xp3"), data).unwrap();
    directory
}
