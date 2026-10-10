//! NeMo SentencePiece vocabulary decoding without retaining transcription tokens.
use super::EngineError;

/// Indexed export vocabulary with one required transducer blank token.
pub(super) struct Vocabulary {
    pieces: Vec<String>,
    spacing: regex::Regex,
    pub blank: usize,
}
impl Vocabulary {
    /// Parses contiguous, unique token IDs using the export's final ID separator.
    pub fn parse(text: &str) -> Result<Self, EngineError> {
        let mut pieces = Vec::new();
        let mut blank = None;
        for line in text.lines() {
            let (piece, id) = line.rsplit_once(' ').ok_or(EngineError::Vocabulary)?;
            if piece.is_empty() || id.parse::<usize>().ok() != Some(pieces.len()) {
                return Err(EngineError::Vocabulary);
            }
            if piece == "<blk>" && blank.replace(pieces.len()).is_some() {
                return Err(EngineError::Vocabulary);
            }
            pieces.push(piece.replace('▁', " "));
        }
        let blank = blank.ok_or(EngineError::Vocabulary)?;
        if pieces.len() < 2 {
            return Err(EngineError::Vocabulary);
        }
        let spacing =
            regex::Regex::new(r"\A\s|\s\B|(\s)\b").map_err(|_| EngineError::Vocabulary)?;
        Ok(Self {
            pieces,
            blank,
            spacing,
        })
    }
    /// Returns the vocabulary logit count, including the blank class.
    pub fn len(&self) -> usize {
        self.pieces.len()
    }
    /// Joins emitted pieces and removes redundant boundary whitespace.
    pub fn decode(&self, tokens: &[usize]) -> Result<String, EngineError> {
        let mut text = String::new();
        for &token in tokens {
            if token == self.blank {
                return Err(EngineError::InvalidOutput);
            }
            text.push_str(self.pieces.get(token).ok_or(EngineError::InvalidOutput)?);
        }
        Ok(self
            .spacing
            .replace_all(
                &text,
                |captures: &regex::Captures<'_>| {
                    if captures.get(1).is_some() {
                        " "
                    } else {
                        ""
                    }
                },
            )
            .into_owned())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    /// Covers Unicode, repeated IDs, missing blank, and invalid emitted tokens.
    #[test]
    fn vocabulary_contract_and_text() -> anyhow::Result<()> {
        let vocab = Vocabulary::parse("▁Ciao 0\n▁città 1\n! 2\n<blk> 3\n")?;
        assert_eq!(vocab.decode(&[0, 1, 2])?, "Ciao città!");
        assert_eq!(vocab.decode(&[])?, "");
        assert!(vocab.decode(&[3]).is_err());
        assert!(vocab.decode(&[4]).is_err());
        let punctuation = Vocabulary::parse("▁Ciao 0\n▁! 1\n<blk> 2\n")?;
        assert_eq!(punctuation.decode(&[0, 1])?, "Ciao!");
        for bad in [
            "",
            "a 0",
            "a 1\n<blk> 2",
            "a 0\nb 0\n<blk> 2",
            "<blk> 0\n<blk> 1",
        ] {
            assert!(Vocabulary::parse(bad).is_err());
        }
        Ok(())
    }
}
