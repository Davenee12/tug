//! Shaping one hold's recording for the accurate engine (whisper.cpp), which takes 32-bit float
//! samples. Pure and tested; the recording is 16 kHz mono 16-bit PCM from `win/capture.rs`.
//!
//! - **Trim**: the silence before the first word and after the last (the keys go down before you
//!   speak and the 200 ms tail runs after), keeping a short margin so no sound is clipped.
//! - **Level**: quiet microphones are brought up so the loudest moment sits near full scale, with
//!   the gain capped so a whisper-quiet recording doesn't become mostly hiss.
//! - **Length**: whisper.cpp ignores anything under a second, so a short "undo" is padded with
//!   silence to just over that.

use super::engine::SAMPLE_RATE;

/// 10 ms frames: what the edges are judged in.
const FRAME: usize = (SAMPLE_RATE / 100) as usize;
/// Kept either side of the speech found (200 ms), so a soft first or last sound survives.
const MARGIN: usize = (SAMPLE_RATE / 5) as usize;
/// A frame is speech when its RMS is at least this share of the loudest frame's...
const RELATIVE: f32 = 0.08;
/// ...and at least this (about -47 dBFS), so a silent room's hiss never counts as speech.
const FLOOR: f32 = 150.0;
/// The loudest sample after levelling, as a share of full scale (headroom against clipping).
const TARGET_PEAK: f32 = 0.9;
/// The most a recording is amplified (about +20 dB).
const MAX_GAIN: f32 = 10.0;
/// The shortest audio handed over: whisper.cpp skips input under 1 s.
pub const MIN_SAMPLES: usize = (SAMPLE_RATE as usize) * 11 / 10;

fn rms(frame: &[i16]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let sum: f64 = frame.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    (sum / frame.len() as f64).sqrt() as f32
}

/// The recording without the silence at its edges (plus [`MARGIN`] either side). All of it when
/// nothing stands out as speech: deciding there's nothing to hear is `judge_recording`'s job.
pub fn trim_silence(pcm: &[i16]) -> &[i16] {
    let levels: Vec<f32> = pcm.chunks(FRAME).map(rms).collect();
    let loudest = levels.iter().copied().fold(0.0f32, f32::max);
    let threshold = (loudest * RELATIVE).max(FLOOR);
    let first = levels.iter().position(|&l| l >= threshold);
    let last = levels.iter().rposition(|&l| l >= threshold);
    match (first, last) {
        (Some(first), Some(last)) => {
            let start = (first * FRAME).saturating_sub(MARGIN);
            let end = ((last + 1) * FRAME + MARGIN).min(pcm.len());
            &pcm[start..end]
        }
        _ => pcm,
    }
}

/// How far the loudest 10 ms must rise above the quiet ones for a recording to hold speech.
const SPEECH_OVER_NOISE: f32 = 2.5;

/// Whether anything in the recording rises above its own background: speech comes and goes, a
/// fan or a hissing microphone doesn't. The quiet level is the 10th-percentile frame (there's
/// always some quiet between words, before the first one and in the tail after letting go).
/// Recognizers invent words from steady noise ("you", "and"), so a hold without this types
/// nothing.
pub fn has_speech(pcm: &[i16]) -> bool {
    let mut levels: Vec<f32> = pcm.chunks(FRAME).map(rms).collect();
    if levels.is_empty() {
        return false;
    }
    levels.sort_by(f32::total_cmp);
    let quiet = levels[levels.len() / 10].max(1.0);
    let loudest = levels[levels.len() - 1];
    loudest >= FLOOR && loudest >= quiet * SPEECH_OVER_NOISE
}

/// The gain that brings the loudest sample to [`TARGET_PEAK`], never below 1 (loud recordings are
/// left alone) and never above [`MAX_GAIN`].
pub fn gain_for(peak: i16) -> f32 {
    let peak = f32::from(peak.saturating_abs()).max(1.0) / 32768.0;
    (TARGET_PEAK / peak).clamp(1.0, MAX_GAIN)
}

/// 16-bit samples as floats in [-1, 1], levelled by [`gain_for`].
pub fn normalize(pcm: &[i16]) -> Vec<f32> {
    let peak = pcm.iter().map(|s| s.saturating_abs()).max().unwrap_or(0);
    let gain = gain_for(peak) / 32768.0;
    pcm.iter().map(|&s| (f32::from(s) * gain).clamp(-1.0, 1.0)).collect()
}

/// Silence added to the end until there's at least [`MIN_SAMPLES`].
pub fn pad(mut samples: Vec<f32>) -> Vec<f32> {
    if samples.len() < MIN_SAMPLES {
        samples.resize(MIN_SAMPLES, 0.0);
    }
    samples
}

/// Everything above, in order: what the accurate engine is given.
pub fn prepare(pcm: &[i16]) -> Vec<f32> {
    pad(normalize(trim_silence(pcm)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ms` of a tone at `amplitude` (a stand-in for speech).
    fn tone(ms: usize, amplitude: f32) -> Vec<i16> {
        let n = ms * SAMPLE_RATE as usize / 1000;
        (0..n).map(|i| (amplitude * (i as f32 * 0.07).sin()) as i16).collect()
    }

    fn quiet(ms: usize, amplitude: i16) -> Vec<i16> {
        let n = ms * SAMPLE_RATE as usize / 1000;
        (0..n)
            .map(|i| if i % 2 == 0 { amplitude } else { -amplitude })
            .collect()
    }

    #[test]
    fn trims_the_silence_at_both_edges_with_a_margin() {
        let mut pcm = quiet(800, 20);
        pcm.extend(tone(500, 8000.0));
        pcm.extend(quiet(900, 20));
        let trimmed = trim_silence(&pcm);
        let speech = 500 * 16;
        assert!(trimmed.len() >= speech, "the speech is all there");
        assert!(
            trimmed.len() <= speech + 2 * MARGIN + 2 * FRAME,
            "about 200 ms kept each side: {} samples",
            trimmed.len()
        );
        // The margin really is silence from either side, not cut into the speech.
        assert!(trimmed[..MARGIN / 2].iter().all(|s| s.abs() <= 20));
        assert!(trimmed[trimmed.len() - MARGIN / 2..].iter().all(|s| s.abs() <= 20));
    }

    #[test]
    fn keeps_everything_when_nothing_stands_out() {
        let hiss = quiet(1000, 60);
        assert_eq!(trim_silence(&hiss).len(), hiss.len(), "below the floor: left alone");
        assert_eq!(trim_silence(&[]).len(), 0);
        let all_speech = tone(700, 9000.0);
        assert_eq!(trim_silence(&all_speech).len(), all_speech.len());
    }

    #[test]
    fn a_soft_word_after_a_loud_one_is_kept() {
        let mut pcm = quiet(300, 10);
        pcm.extend(tone(300, 12000.0));
        pcm.extend(quiet(200, 10));
        pcm.extend(tone(300, 1500.0)); // 12.5% of the loud one: still speech
        pcm.extend(quiet(600, 10));
        let trimmed = trim_silence(&pcm);
        assert!(trimmed.len() >= (300 + 200 + 300) * 16, "{}", trimmed.len());
    }

    #[test]
    fn levels_quiet_speech_up_with_a_cap() {
        assert_eq!(gain_for(i16::MAX), 1.0, "loud: untouched");
        assert_eq!(gain_for(i16::MIN), 1.0);
        let g = gain_for(3277); // -20 dBFS
        assert!((g - 9.0).abs() < 0.05, "{g}");
        assert_eq!(gain_for(100), MAX_GAIN, "very quiet: capped");
        assert_eq!(gain_for(0), MAX_GAIN, "silence doesn't divide by zero");

        let out = normalize(&[3277, -3277, 0]);
        assert!((out[0] - 0.9).abs() < 0.01, "{}", out[0]);
        assert!((out[1] + 0.9).abs() < 0.01);
        assert_eq!(out[2], 0.0);
        let loud = normalize(&[i16::MAX, i16::MIN]);
        assert!(loud.iter().all(|s| (-1.0..=1.0).contains(s)), "never past full scale");
    }

    #[test]
    fn short_recordings_are_padded_past_a_second() {
        assert_eq!(pad(vec![0.5; 100]).len(), MIN_SAMPLES);
        assert_eq!(pad(vec![0.5; 100])[0], 0.5, "the sound comes first");
        assert_eq!(
            pad(vec![0.1; MIN_SAMPLES + 7]).len(),
            MIN_SAMPLES + 7,
            "long ones untouched"
        );
        assert!(MIN_SAMPLES > SAMPLE_RATE as usize);
    }

    #[test]
    fn speech_rises_above_the_noise_and_steady_noise_doesnt() {
        let mut speech = quiet(300, 20);
        speech.extend(tone(600, 6000.0));
        speech.extend(quiet(300, 20));
        assert!(has_speech(&speech));

        // The same words over a hissing microphone: still well above it.
        let mut noisy_speech = quiet(300, 600);
        noisy_speech.extend(tone(600, 6000.0));
        noisy_speech.extend(quiet(300, 600));
        assert!(has_speech(&noisy_speech));

        // Hiss (or a fan) and nothing else: loud enough to pass the silence check, but flat.
        assert!(!has_speech(&quiet(1500, 600)), "steady hiss");
        assert!(!has_speech(&tone(1500, 3000.0)), "a steady hum");
        assert!(!has_speech(&quiet(1000, 50)), "near silence");
        assert!(!has_speech(&[]));
    }

    #[test]
    fn prepares_a_hold_end_to_end() {
        let mut pcm = quiet(500, 15);
        pcm.extend(tone(400, 2000.0));
        pcm.extend(quiet(500, 15));
        let out = prepare(&pcm);
        assert_eq!(out.len(), MIN_SAMPLES, "trimmed to ~0.8 s, then padded");
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.5 && peak <= 0.91, "levelled: {peak}");
    }
}
