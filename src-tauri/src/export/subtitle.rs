use crate::transcription::SubtitleSegment;
use serde::Deserialize;

/// Supported export formats; arbitrary format strings are rejected by IPC.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Txt,
    Srt,
    Vtt,
}
impl ExportFormat {
    /// Returns the native dialog extension for this format.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Srt => "srt",
            Self::Vtt => "vtt",
        }
    }
}
/// Validates finite, nonoverlapping timestamps before persistence or export.
pub fn validate_segments(segments: &[SubtitleSegment]) -> Result<(), String> {
    let mut end = 0.0;
    for segment in segments {
        if !segment.start_sec.is_finite()
            || !segment.end_sec.is_finite()
            || segment.start_sec < end
            || segment.end_sec <= segment.start_sec
            || (segment.end_sec * 1000.0).round() <= (segment.start_sec * 1000.0).round()
            || segment.end_sec > 359_999.999
            || segment.text.contains('\0')
        {
            return Err("Invalid transcript segments or timestamps.".into());
        }
        end = segment.end_sec;
    }
    Ok(())
}
/// Formats rounded milliseconds with the format-specific decimal separator.
fn timestamp(seconds: f64, separator: char) -> String {
    let millis = (seconds * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}{separator}{:03}",
        millis / 3_600_000,
        (millis / 60_000) % 60,
        (millis / 1000) % 60,
        millis % 1000
    )
}
/// Produces plain subtitle text, preventing markup and blank cue separators.
fn cue_text(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
/// Formats edited text, skips empty segments, and renumbers subtitle cues.
pub fn format_segments(
    segments: &[SubtitleSegment],
    format: ExportFormat,
) -> Result<String, String> {
    validate_segments(segments)?;
    let nonempty: Vec<_> = segments
        .iter()
        .filter(|s| !s.text.trim().is_empty())
        .collect();
    if matches!(format, ExportFormat::Txt) {
        return Ok(nonempty
            .iter()
            .map(|s| s.text.trim())
            .collect::<Vec<_>>()
            .join("\n\n"));
    }
    let mut output = if matches!(format, ExportFormat::Vtt) {
        "WEBVTT\n\n".into()
    } else {
        String::new()
    };
    let separator = if matches!(format, ExportFormat::Srt) {
        ','
    } else {
        '.'
    };
    for (index, segment) in nonempty.into_iter().enumerate() {
        output.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            index + 1,
            timestamp(segment.start_sec, separator),
            timestamp(segment.end_sec, separator),
            cue_text(&segment.text)
        ));
    }
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    /// Covers hour boundaries, Unicode, empty cues, multiline text and unsafe timestamps.
    #[test]
    fn subtitles_preserve_edited_text_and_validate_time() -> Result<(), String> {
        let mut segments = vec![
            SubtitleSegment {
                start_sec: 0.0,
                end_sec: 1.0,
                text: " ".into(),
            },
            SubtitleSegment {
                start_sec: 3599.9996,
                end_sec: 3601.2,
                text: "Città <b>\n\nhello".into(),
            },
        ];
        let srt = format_segments(&segments, ExportFormat::Srt)?;
        assert!(srt.starts_with("1\n01:00:00,000 --> 01:00:01,200"));
        assert!(srt.contains("Città &lt;b&gt;\nhello"));
        assert!(
            format_segments(&segments, ExportFormat::Vtt)?.starts_with("WEBVTT\n\n1\n01:00:00.000")
        );
        assert_eq!(
            format_segments(&segments, ExportFormat::Txt)?,
            "Città <b>\n\nhello"
        );
        segments[1].start_sec = f64::NAN;
        assert!(validate_segments(&segments).is_err());
        segments[1].start_sec = 0.5;
        assert!(validate_segments(&segments).is_err());
        segments.truncate(1);
        segments[0].end_sec = 0.0001;
        assert!(validate_segments(&segments).is_err());
        Ok(())
    }
}
