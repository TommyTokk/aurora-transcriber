//! Presentation blocks assembled only after successful chunk inference.
use super::SubtitleSegment;

pub(super) fn group(segments: Vec<SubtitleSegment>, duration: f64) -> Vec<SubtitleSegment> {
    if segments.is_empty() {
        return segments;
    }
    let target = if duration <= 1800.0 { 600.0 } else { 1200.0 };
    let mut chunks = segments.into_iter().peekable();
    let mut blocks = Vec::new();
    let mut start = 0.0;
    while chunks.peek().is_some() {
        let goal = start + target;
        let mut texts = Vec::new();
        let mut end;
        while let Some(chunk) = chunks.next() {
            end = chunk.end_sec;
            let text = chunk.text.trim().to_owned();
            if !text.is_empty() {
                texts.push(text);
            }
            let stop = chunks
                .peek()
                .is_none_or(|next| (end - goal).abs() <= (next.end_sec - goal).abs());
            if stop {
                if chunks.peek().is_none() {
                    end = duration;
                }
                blocks.push(SubtitleSegment {
                    start_sec: start,
                    end_sec: end,
                    text: texts.join(" "),
                });
                start = end;
                break;
            }
        }
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    fn chunks(ends: &[f64]) -> Vec<SubtitleSegment> {
        let mut start = 0.0;
        ends.iter()
            .enumerate()
            .map(|(i, &end)| {
                let chunk = SubtitleSegment {
                    start_sec: start,
                    end_sec: end,
                    text: format!("word{i}"),
                };
                start = end;
                chunk
            })
            .collect()
    }
    #[test]
    fn thresholds_and_nearest_boundaries() {
        let ends = [590.0, 610.0, 1190.0, 1210.0, 1790.0];
        let blocks = group(chunks(&ends), 1800.0);
        assert_eq!(
            blocks.iter().map(|b| b.end_sec).collect::<Vec<_>>(),
            [590.0, 1190.0, 1800.0]
        );
        assert_eq!(blocks[0].text, "word0");
        assert_eq!(blocks[1].text, "word1 word2");
        let long = group(chunks(&ends), 1800.01);
        assert_eq!(long.len(), 2);
        assert_eq!(long[0].end_sec, 1190.0);
        assert_eq!(long[1].end_sec, 1800.01);
        assert!(blocks.windows(2).all(|b| b[0].end_sec == b[1].start_sec));
    }
    #[test]
    fn short_silent_and_sparse_audio() {
        assert!(group(vec![], 3600.0).is_empty());
        let short = group(chunks(&[5.0]), 90.0);
        assert_eq!((short[0].start_sec, short[0].end_sec), (0.0, 90.0));
        let sparse = group(chunks(&[10.0, 1700.0]), 1800.0);
        assert_eq!(sparse.last().map(|b| b.end_sec), Some(1800.0));
        assert_eq!(
            sparse
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            "word0 word1"
        );
        let mut empty = chunks(&[5.0, 8.0]);
        empty[0].text = "  ".into();
        assert_eq!(group(empty, 10.0)[0].text, "word1");
    }
}
