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

// 🦀 The old WAV-encoding helpers (f32_to_i16 / encode_wav_pcm16_16k_mono / window_to_wav) were
//    removed with the HTTP whisper-server path — whisper-rs consumes raw 16 kHz mono f32 directly,
//    so the two helpers above are all the in-process pipeline needs.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_stereo_and_passes_mono() {
        assert_eq!(downmix_to_mono(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(downmix_to_mono(&[0.1, 0.2, 0.3], 1), vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn resample_passes_16k_through_and_resizes_48k() {
        assert_eq!(resample_to_16k(&vec![0.0f32; 100], 16_000).len(), 100);
        let out = resample_to_16k(&vec![0.0f32; 4800], 48_000); // 0.1s @48k → ~0.1s @16k
        assert!((out.len() as i64 - 1600).abs() <= 2, "got {}", out.len());
    }
}
