use std::io::Cursor;

const MAX_SAMPLES: usize = 48_000 * 2 * 600;

pub fn wav(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut ogg = ogg::reading::PacketReader::new(Cursor::new(bytes));
    let first = ogg
        .read_packet()
        .map_err(|e| e.to_string())?
        .ok_or("empty Ogg stream")?;
    let (samples, rate, channels) = if first.data.starts_with(b"OpusHead") {
        if first.data.len() < 19 || first.data[8] > 15 || first.data[18] != 0 {
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
        if !tags.data.starts_with(b"OpusTags") {
            return Err("missing OpusTags".into());
        }
        let mut decoder = opus::Decoder::new(48000, kind).map_err(|e| e.to_string())?;
        let mut output = Vec::new();
        let mut buffer = vec![0i16; 5760 * channels];
        let mut final_granule = None;
        while let Some(packet) = ogg.read_packet().map_err(|e| e.to_string())? {
            let count = decoder
                .decode(&packet.data, &mut buffer, false)
                .map_err(|e| e.to_string())?
                * channels;
            if output.len() + count > MAX_SAMPLES {
                return Err("audio exceeds ten-minute decode budget".into());
            }
            output.extend(buffer[..count].iter().map(|v| {
                (*v as f32 * gain)
                    .round()
                    .clamp(i16::MIN as f32, i16::MAX as f32) as i16
            }));
            if packet.last_in_stream() {
                final_granule = Some(
                    usize::try_from(packet.absgp_page()).map_err(|_| "Opus granule overflow")?,
                );
            }
        }
        let end = final_granule
            .ok_or("truncated Opus stream: EOS missing")?
            .checked_mul(channels)
            .ok_or("Opus length overflow")?
            .min(output.len());
        let start = preskip
            .checked_mul(channels)
            .ok_or("Opus preskip overflow")?;
        if start > end {
            return Err("invalid Opus preskip".into());
        }
        (output[start..end].to_vec(), 48000u32, channels as u16)
    } else if first.data.starts_with(b"\x01vorbis") {
        let mut reader = lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes))
            .map_err(|e| e.to_string())?;
        let channels = reader.ident_hdr.audio_channels as u16;
        let rate = reader.ident_hdr.audio_sample_rate;
        if channels == 0 || channels > 2 || rate == 0 {
            return Err("unsupported Vorbis format".into());
        }
        let mut samples = Vec::new();
        while let Some(packet) = reader.read_dec_packet_itl().map_err(|e| e.to_string())? {
            if samples.len() + packet.len() > MAX_SAMPLES {
                return Err("audio exceeds decode budget".into());
            }
            samples.extend(packet);
        }
        (samples, rate, channels)
    } else {
        return Err("not Opus or Vorbis".into());
    };
    pcm_wav(&samples, rate, channels)
}

fn pcm_wav(samples: &[i16], rate: u32, channels: u16) -> Result<Vec<u8>, String> {
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
