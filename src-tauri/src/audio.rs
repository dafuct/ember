
pub fn downmix_to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Root-mean-square amplitude of a mono buffer — a cheap loudness proxy.
///
/// The live pipeline uses this to skip near-silent windows before they reach
/// Whisper: fed silence, the model hallucinates its most common training phrase
/// (for Ukrainian YouTube data that is "Дякую за перегляд!"), which is exactly
/// the garbage that flooded early recordings.
// 🦀 f64::from(s) widens each f32 to f64 so the squares don't lose precision when
// summed over a 30-second window (480 000 samples); we narrow back to f32 at the end.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    (sum_sq / samples.len() as f64).sqrt() as f32
}

pub fn resample_to_16k(mono: &[f32], in_rate: u32) -> Vec<f32> {
    const OUT_RATE: u32 = 16_000;
    if in_rate == OUT_RATE || mono.len() < 2 {
        return mono.to_vec();
    }
    let ratio = OUT_RATE as f64 / in_rate as f64;
    let out_len = ((mono.len() as f64) * ratio).round() as usize;
    if out_len == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 / ratio;
        let i0 = src.floor() as usize;
        let i1 = (i0 + 1).min(mono.len() - 1);
        let frac = (src - i0 as f64) as f32;
        out.push(mono[i0] * (1.0 - frac) + mono[i1] * frac);
    }
    out
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
    fn rms_zero_for_silence_and_full_scale_for_square_wave() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&[0.0; 128]), 0.0);
        // A full-scale ±1.0 square wave has RMS exactly 1.0.
        assert!((rms(&[1.0, -1.0, 1.0, -1.0]) - 1.0).abs() < 1e-6);
        // A very quiet signal sits well under a speech-level gate.
        assert!(rms(&[0.002, -0.002, 0.002, -0.002]) < 0.005);
    }

    #[test]
    fn resample_passes_16k_through_and_resizes_48k() {
        assert_eq!(resample_to_16k(&vec![0.0f32; 100], 16_000).len(), 100);
        let out = resample_to_16k(&vec![0.0f32; 4800], 48_000);
        assert!((out.len() as i64 - 1600).abs() <= 2, "got {}", out.len());
    }
}
