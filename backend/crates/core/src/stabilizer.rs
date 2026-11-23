//! Per-region OCR subtitle text stabilization, noise suppression, and feedback filtering.

use std::collections::HashMap;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct StabilizerConfig {
    /// Number of consecutive matching observations required before emitting a subtitle.
    pub required_confirmations: u32,
    /// Minimum OCR confidence score accepted as valid dialogue.
    pub min_confidence: f32,
    /// Normalized character similarity threshold above which minor OCR jitter is ignored.
    pub noise_similarity_threshold: f32,
}

impl Default for StabilizerConfig {
    fn default() -> Self {
        Self {
            required_confirmations: 2,
            min_confidence: 0.45,
            noise_similarity_threshold: 0.90,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StabilizedSubtitle {
    pub region_id: String,
    pub sequence: u64,
    pub text: String,
    pub normalized_text: String,
    pub stabilization_ms: u64,
}

#[derive(Debug, Default)]
struct RegionStabilityState {
    candidate_raw: String,
    candidate_norm: String,
    candidate_first_seen: Option<Instant>,
    confirmations: u32,
    last_emitted_norm: Option<String>,
    next_sequence: u64,
}

pub struct OcrStabilizer {
    config: StabilizerConfig,
    regions: HashMap<String, RegionStabilityState>,
    last_displayed_vietnamese_norm: Option<String>,
}

impl OcrStabilizer {
    pub fn new(config: StabilizerConfig) -> Self {
        Self {
            config,
            regions: HashMap::new(),
            last_displayed_vietnamese_norm: None,
        }
    }

    /// Record the currently displayed Vietnamese overlay text so that if WGC capture exclusion
    /// is unavailable on an older setup, the stabilizer refuses to re-OCR its own subtitle.
    pub fn set_displayed_overlay_text(&mut self, vietnamese_text: Option<&str>) {
        self.last_displayed_vietnamese_norm = vietnamese_text
            .map(normalize_subtitle_text)
            .filter(|s| !s.is_empty());
    }

    pub fn remove_region(&mut self, region_id: &str) {
        self.regions.remove(region_id);
    }

    pub fn reset_all(&mut self) {
        self.regions.clear();
        self.last_displayed_vietnamese_norm = None;
    }

    /// Observe raw OCR output for a region. Returns `Some(StabilizedSubtitle)` only when
    /// text has stabilized, is not minor OCR noise over the currently active subtitle,
    /// and is not self-captured overlay text.
    pub fn observe(
        &mut self,
        region_id: &str,
        raw_text: &str,
        confidence: f32,
    ) -> Option<StabilizedSubtitle> {
        let state = self.regions.entry(region_id.to_string()).or_default();
        let trimmed = raw_text.trim();

        // Empty frame clears active subtitle state so that if the exact same dialogue line
        // genuinely appears again later in the conversation, it CAN be displayed and spoken
        // again (using the persistent translation cache without hitting cloud APIs).
        if trimmed.is_empty() || confidence < self.config.min_confidence {
            state.candidate_raw.clear();
            state.candidate_norm.clear();
            state.candidate_first_seen = None;
            state.confirmations = 0;
            if trimmed.is_empty() {
                state.last_emitted_norm = None;
            }
            return None;
        }

        let norm = normalize_subtitle_text(trimmed);
        if norm.is_empty() {
            return None;
        }

        // Guard against OCRing our own Vietnamese overlay when capture exclusion fails.
        if let Some(overlay_norm) = &self.last_displayed_vietnamese_norm {
            if norm == *overlay_norm || normalized_similarity(&norm, overlay_norm) >= 0.88 {
                return None;
            }
        }

        // Suppress unchanged or insignificant OCR noise relative to currently displayed line.
        if let Some(last_emitted) = &state.last_emitted_norm {
            if norm == *last_emitted {
                return None;
            }
            if is_insignificant_ocr_noise(
                last_emitted,
                &norm,
                self.config.noise_similarity_threshold,
            ) {
                return None;
            }
        }

        // Typewriter / gradual dialogue appearance handling:
        // If new text extends previous candidate (prefix growth), reset confirmation counter
        // so we wait until the full sentence finishes appearing before translating.
        let matches_candidate = !state.candidate_norm.is_empty()
            && (norm == state.candidate_norm
                || is_insignificant_ocr_noise(
                    &state.candidate_norm,
                    &norm,
                    self.config.noise_similarity_threshold,
                ));

        if matches_candidate {
            state.confirmations += 1;
            // Keep cleaner / longer representation if nearly identical
            if trimmed.chars().count() >= state.candidate_raw.chars().count() {
                state.candidate_raw = trimmed.to_string();
                state.candidate_norm = norm.clone();
            }
        } else {
            state.candidate_raw = trimmed.to_string();
            state.candidate_norm = norm;
            state.candidate_first_seen = Some(Instant::now());
            state.confirmations = 1;
        }

        if state.confirmations >= self.config.required_confirmations {
            state.next_sequence += 1;
            let seq = state.next_sequence;
            let stab_ms = state
                .candidate_first_seen
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0);
            state.last_emitted_norm = Some(state.candidate_norm.clone());
            return Some(StabilizedSubtitle {
                region_id: region_id.to_string(),
                sequence: seq,
                text: state.candidate_raw.clone(),
                normalized_text: state.candidate_norm.clone(),
                stabilization_ms: stab_ms,
            });
        }

        None
    }

    /// Force- query whether a candidate is currently waiting to stabilize (used by scheduler
    /// so frame-diff doesn't skip the confirmation check when pixels just stopped changing!).
    pub fn has_pending_candidate(&self, region_id: &str) -> bool {
        self.regions
            .get(region_id)
            .map(|s| {
                !s.candidate_norm.is_empty()
                    && s.confirmations < self.config.required_confirmations
                    && s.last_emitted_norm.as_ref() != Some(&s.candidate_norm)
            })
            .unwrap_or(false)
    }
}

pub fn normalize_subtitle_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_space = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
                prev_space = true;
            }
            continue;
        }
        prev_space = false;
        // Normalize fullwidth ASCII punctuation variants common in CJK OCR
        let mapped = match ch {
            '！' => '!',
            '？' => '?',
            '，' => ',',
            '．' => '.',
            '：' => ':',
            '；' => ';',
            '“' | '”' => '"',
            '‘' | '’' => '\'',
            _ => ch,
        };
        for lower in mapped.to_lowercase() {
            out.push(lower);
        }
    }
    out.trim().to_string()
}

fn is_insignificant_ocr_noise(a: &str, b: &str, threshold: f32) -> bool {
    let len_a = a.chars().count();
    let len_b = b.chars().count();
    let max_len = len_a.max(len_b);
    if max_len < 6 {
        return a == b;
    }
    // If one is a strict prefix and significantly shorter, it's typewriter growth, NOT noise.
    if (b.starts_with(a) || a.starts_with(b)) && len_a.abs_diff(len_b) >= 2 {
        return false;
    }
    normalized_similarity(a, b) >= threshold
}

pub fn normalized_similarity(a: &str, b: &str) -> f32 {
    if a == b {
        return 1.0;
    }
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let max_len = a_chars.len().max(b_chars.len());
    if max_len == 0 {
        return 1.0;
    }
    let dist = levenshtein_chars(&a_chars, &b_chars);
    1.0 - (dist as f32 / max_len as f32)
}

fn levenshtein_chars(a: &[char], b: &[char]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];

    for (i, &ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_gradually_appearing_typewriter_text_to_stabilize() {
        let mut stab = OcrStabilizer::new(StabilizerConfig::default());

        assert!(stab.observe("r1", "また", 0.95).is_none());
        assert!(stab.observe("r1", "また会えて", 0.95).is_none());
        assert!(stab.observe("r1", "また会えて嬉しい。", 0.96).is_none());
        assert!(stab.has_pending_candidate("r1"));

        let emitted = stab
            .observe("r1", "また会えて嬉しい。", 0.97)
            .expect("should emit once stable");
        assert_eq!(emitted.sequence, 1);
        assert_eq!(emitted.text, "また会えて嬉しい。");

        // Unchanged subsequent observations do not re-emit
        assert!(stab.observe("r1", "また会えて嬉しい。", 0.98).is_none());
    }

    #[test]
    fn ignores_minor_ocr_character_jitter_on_long_subtitles() {
        let mut stab = OcrStabilizer::new(StabilizerConfig::default());
        assert!(stab
            .observe("r1", "Welcome back to the hidden village!", 0.95)
            .is_none());
        let first = stab
            .observe("r1", "Welcome back to the hidden village!", 0.96)
            .unwrap();
        assert_eq!(first.sequence, 1);

        // Single-char punctuation/letter flicker (>90% similarity) is suppressed
        assert!(stab
            .observe("r1", "Welcome back to the hidden village.", 0.91)
            .is_none());
    }

    #[test]
    fn allows_repeated_sentence_after_blank_gap_and_maintains_independent_regions() {
        let mut stab = OcrStabilizer::new(StabilizerConfig::default());

        let _ = stab.observe("r1", "Hello there!", 0.95);
        let first = stab.observe("r1", "Hello there!", 0.95).unwrap();
        assert_eq!(first.sequence, 1);

        // Region 2 has completely independent sequence and state
        let _ = stab.observe("r2", "Hello there!", 0.95);
        let r2_first = stab.observe("r2", "Hello there!", 0.95).unwrap();
        assert_eq!(r2_first.sequence, 1);

        // Blank subtitle clears r1's active line so later reappearance emits sequence 2
        assert!(stab.observe("r1", "", 0.0).is_none());
        let _ = stab.observe("r1", "Hello there!", 0.95);
        let second = stab.observe("r1", "Hello there!", 0.95).unwrap();
        assert_eq!(second.sequence, 2);
    }

    #[test]
    fn prevents_self_ocr_feedback_when_overlay_capture_exclusion_unavailable() {
        let mut stab = OcrStabilizer::new(StabilizerConfig::default());
        stab.set_displayed_overlay_text(Some("Rất vui khi được gặp lại bạn."));

        assert!(stab
            .observe("r1", "Rất vui khi được gặp lại bạn.", 0.99)
            .is_none());
        assert!(stab
            .observe("r1", "Rất vui khi được gặp lại bạn.", 0.99)
            .is_none());
    }
}
