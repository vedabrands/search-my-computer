use super::Chunk;
use crate::extractor::ExtractedDoc;

#[derive(Debug, Clone)]
pub struct ProseChunkerConfig {
    /// Target chunk size in characters (~250 tokens ≈ 1000 characters).
    pub target_chars: usize,
    /// Overlap in characters (~15% of target ≈ 150 characters).
    pub overlap_chars: usize,
    /// Minimum chunk size to avoid tiny trailing chunks.
    pub min_chunk_chars: usize,
}

impl Default for ProseChunkerConfig {
    fn default() -> Self {
        Self {
            target_chars: 1000,
            overlap_chars: 150,
            min_chunk_chars: 100,
        }
    }
}

pub struct ProseChunker {
    config: ProseChunkerConfig,
}

impl ProseChunker {
    pub fn new(config: ProseChunkerConfig) -> Self {
        Self { config }
    }

    pub fn chunk_doc(&self, doc: &ExtractedDoc) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut ordinal = 0;

        for block in &doc.blocks {
            let text = block.text.trim();
            if text.is_empty() {
                continue;
            }

            // If the block is within reasonable chunk size, keep it as a single chunk
            if text.len() <= self.config.target_chars + self.config.overlap_chars {
                chunks.push(Chunk::new(
                    ordinal,
                    text,
                    block.page,
                    block.section.clone(),
                    None,
                    block.start_offset,
                    block.end_offset,
                ));
                ordinal += 1;
            } else {
                // Split large block into sentence/paragraph-bounded chunks with overlap
                let sub_chunks = self.split_text_with_overlap(
                    text,
                    block.page,
                    block.section.as_deref(),
                    block.start_offset,
                    &mut ordinal,
                );
                chunks.extend(sub_chunks);
            }
        }

        chunks
    }

    fn split_text_with_overlap(
        &self,
        text: &str,
        page: Option<usize>,
        section: Option<&str>,
        base_offset: usize,
        ordinal: &mut usize,
    ) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let sentences = split_sentences(text);

        let mut current_chunk = Vec::new();
        let mut current_len = 0;
        let mut chunk_start_offset = 0;

        for (sent_text, start_idx, end_idx) in sentences {
            if current_chunk.is_empty() {
                chunk_start_offset = start_idx;
            }

            current_chunk.push(sent_text);
            current_len += sent_text.len();

            if current_len >= self.config.target_chars {
                let joined_text = current_chunk.join(" ");
                let chunk_end_offset = end_idx;

                chunks.push(Chunk::new(
                    *ordinal,
                    joined_text,
                    page,
                    section.map(String::from),
                    None,
                    base_offset + chunk_start_offset,
                    base_offset + chunk_end_offset,
                ));
                *ordinal += 1;

                // Create overlap window: keep sentences from the end that fit in overlap_chars
                let mut overlap_sentences = Vec::new();
                let mut overlap_len = 0;

                for &s in current_chunk.iter().rev() {
                    if overlap_len + s.len() <= self.config.overlap_chars
                        || overlap_sentences.is_empty()
                    {
                        overlap_sentences.push(s);
                        overlap_len += s.len();
                    } else {
                        break;
                    }
                }
                overlap_sentences.reverse();

                // Advance chunk start to start of the first sentence in overlap
                chunk_start_offset = chunk_end_offset.saturating_sub(overlap_len);
                current_chunk = overlap_sentences;
                current_len = overlap_len;
            }
        }

        // Add remaining sentences if any
        if !current_chunk.is_empty() {
            let joined_text = current_chunk.join(" ");
            if !joined_text.trim().is_empty() {
                // If the last chunk is too small and we already have chunks, append to last if feasible
                if joined_text.len() < self.config.min_chunk_chars && !chunks.is_empty() {
                    let last = chunks.last_mut().unwrap();
                    last.text.push(' ');
                    last.text.push_str(&joined_text);
                    last.end_offset = base_offset + text.len();
                } else {
                    chunks.push(Chunk::new(
                        *ordinal,
                        joined_text,
                        page,
                        section.map(String::from),
                        None,
                        base_offset + chunk_start_offset,
                        base_offset + text.len(),
                    ));
                    *ordinal += 1;
                }
            }
        }

        chunks
    }
}

impl Default for ProseChunker {
    fn default() -> Self {
        Self::new(ProseChunkerConfig::default())
    }
}

/// Split text into sentences with start and end character offsets.
fn split_sentences(text: &str) -> Vec<(&str, usize, usize)> {
    let mut sentences = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();

    if chars.is_empty() {
        return sentences;
    }

    let len = chars.len();
    for i in 0..len {
        let (idx, ch) = chars[i];
        let is_sentence_end = ch == '.' || ch == '!' || ch == '?' || ch == '\n';

        if is_sentence_end {
            // Check if next char is whitespace or end of string
            let is_boundary = if i + 1 < len {
                let (_, next_ch) = chars[i + 1];
                next_ch.is_whitespace()
            } else {
                true
            };

            if is_boundary {
                let sentence_slice = text[start..=idx].trim();
                if !sentence_slice.is_empty() {
                    sentences.push((sentence_slice, start, idx + 1));
                }
                start = if i + 1 < len {
                    chars[i + 1].0
                } else {
                    text.len()
                };
            }
        }
    }

    if start < text.len() {
        let sentence_slice = text[start..].trim();
        if !sentence_slice.is_empty() {
            sentences.push((sentence_slice, start, text.len()));
        }
    }

    sentences
}
