// src-tauri/src/audio.rs — pure audio helpers for live capture (no I/O, unit-testable, M24).

/// Average all channels of an interleaved frame buffer down to mono.
/// `interleaved` is [L,R,L,R,…] for 2 channels; returns one sample per frame.
pub fn downmix_to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return interleaved.to_vec();
    }
    // 🦀 `chunks_exact(ch)` yields one slice per frame; a trailing partial frame is dropped.
    interleaved
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Linear-interpolation resample a mono buffer from `in_rate` to 16 000 Hz.
/// Passthrough when already 16 kHz (or too short). Handles arbitrary ratios (e.g. 44.1 kHz).
pub fn resample_to_16k(mono: &[f32], in_rate: u32) -> Vec<f32> {
    const OUT_RATE: u32 = 16_000;
    if in_rate == OUT_RATE || mono.len() < 2 {
        return mono.to_vec();
    }
    let ratio = OUT_RATE as f64 / in_rate as f64;
    let out_len = ((mono.len() as f64) * ratio).round() as usize;
    // 🦀 A very short input at a high in_rate can round to zero output samples; return empty
    //    explicitly rather than relying on the loop never running.
    if out_len == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        // 🦀 `src` is the fractional sample position in the input; lerp between its neighbours.
        let src = i as f64 / ratio;
        let i0 = src.floor() as usize;
        let i1 = (i0 + 1).min(mono.len() - 1);
        let frac = (src - i0 as f64) as f32;
        out.push(mono[i0] * (1.0 - frac) + mono[i1] * frac);
    }
    out
}

/// Convert f32 samples in [-1.0, 1.0] to 16-bit PCM (clamp then scale).
/// NaN inputs are saturated to 0 via Rust's `as i16` cast semantics.
pub fn f32_to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect()
}

/// Encode 16 kHz mono 16-bit PCM as a WAV byte buffer (44-byte RIFF/WAVE header + data).
// 🦀 A WAV file is a RIFF container: a "RIFF"+size+"WAVE" header, a "fmt " chunk describing the
//    format, then a "data" chunk of little-endian samples. We write the 44 bytes by hand.
pub fn encode_wav_pcm16_16k_mono(samples: &[i16]) -> Vec<u8> {
    const SAMPLE_RATE: u32 = 16_000;
    const CHANNELS: u16 = 1;
    const BITS: u16 = 16;
    let data_len = (samples.len() * 2) as u32; // 2 bytes per i16 sample
    let byte_rate = SAMPLE_RATE * CHANNELS as u32 * (BITS as u32 / 8);
    let block_align = CHANNELS * (BITS / 8);
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes()); // RIFF chunk size = 36 + data
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size (PCM)
    v.extend_from_slice(&1u16.to_le_bytes()); // audio format = 1 (PCM)
    v.extend_from_slice(&CHANNELS.to_le_bytes());
    v.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    v.extend_from_slice(&byte_rate.to_le_bytes());
    v.extend_from_slice(&block_align.to_le_bytes());
    v.extend_from_slice(&BITS.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

/// Full window pipeline: interleaved capture frames → mono → 16 kHz → PCM16 → WAV bytes.
pub fn window_to_wav(interleaved: &[f32], channels: u16, in_rate: u32) -> Vec<u8> {
    let mono = downmix_to_mono(interleaved, channels);
    let resampled = resample_to_16k(&mono, in_rate);
    let pcm = f32_to_i16(&resampled);
    encode_wav_pcm16_16k_mono(&pcm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_stereo_and_passes_mono() {
        assert_eq!(downmix_to_mono(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(downmix_to_mono(&[0.1, 0.2, 0.3], 1), vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn f32_to_i16_clamps_and_scales() {
        assert_eq!(f32_to_i16(&[0.0, 1.0, -1.0, 2.0, -2.0]), vec![0, 32767, -32767, 32767, -32767]);
    }

    #[test]
    fn resample_passes_16k_through_and_resizes_48k() {
        assert_eq!(resample_to_16k(&vec![0.0f32; 100], 16_000).len(), 100);
        let out = resample_to_16k(&vec![0.0f32; 4800], 48_000); // 0.1s @48k → ~0.1s @16k
        assert!((out.len() as i64 - 1600).abs() <= 2, "got {}", out.len());
    }

    #[test]
    fn window_to_wav_chains_to_a_valid_wav() {
        // 2ch @ 48k, 4 frames (8 interleaved samples) → mono → 16k → PCM16 → WAV.
        let wav = window_to_wav(&[0.0, 0.0, 0.5, 0.5, -0.5, -0.5, 1.0, 1.0], 2, 48_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        // header is always 44 bytes; body length must be even (2 bytes/sample)
        assert!(wav.len() >= 44);
        assert_eq!((wav.len() - 44) % 2, 0);
    }

    #[test]
    fn wav_header_is_well_formed() {
        let wav = encode_wav_pcm16_16k_mono(&[0, 1, -1]);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1); // channels
        assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), 16_000); // rate
        assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16); // bits
        assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]), 6); // data len = 3*2
        assert_eq!(wav.len(), 44 + 6);
    }
}
