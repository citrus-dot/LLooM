//! Decode SSE lines at byte boundaries; HTTP chunks can split UTF-8 code points.

use crate::error::{AppError, Result};

const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Default)]
pub(super) struct LineDecoder {
    pending: Vec<u8>,
}

impl LineDecoder {
    pub(super) fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>> {
        let mut lines = Vec::new();
        for part in chunk.split_inclusive(|&b| b == b'\n') {
            if self.pending.len() + part.len() > MAX_LINE_BYTES {
                return Err(AppError::AiService("provider SSE 单行超过 4 MiB".into()));
            }
            self.pending.extend_from_slice(part);
            if self.pending.last() == Some(&b'\n') {
                self.pending.pop();
                if self.pending.last() == Some(&b'\r') {
                    self.pending.pop();
                }
                lines.push(decode(&self.pending)?);
                self.pending.clear();
            }
        }
        Ok(lines)
    }

    /// Some compatible endpoints omit the final newline. A valid final data
    /// line still needs to be delivered before the stream is marked done.
    pub(super) fn finish(&mut self) -> Result<Option<String>> {
        if self.pending.is_empty() {
            return Ok(None);
        }
        if self.pending.last() == Some(&b'\r') {
            self.pending.pop();
        }
        let line = decode(&self.pending)?;
        self.pending.clear();
        Ok(Some(line))
    }
}

fn decode(bytes: &[u8]) -> Result<String> {
    String::from_utf8(bytes.to_vec())
        .map_err(|e| AppError::AiService(format!("provider SSE 包含无效 UTF-8: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_codepoints_survive_chunk_boundaries_and_final_line() {
        let mut decoder = LineDecoder::default();
        let text = "data: {\"content\":\"你好👋\"}\r\ndata: {\"tail\":true}";
        let mut lines = Vec::new();
        for byte in text.as_bytes() {
            lines.extend(decoder.push(&[*byte]).unwrap());
        }
        assert_eq!(lines, ["data: {\"content\":\"你好👋\"}"]);
        assert_eq!(
            decoder.finish().unwrap().as_deref(),
            Some("data: {\"tail\":true}")
        );
    }

    #[test]
    fn rejects_oversize_and_invalid_lines() {
        let mut decoder = LineDecoder::default();
        assert!(decoder.push(&vec![b'a'; MAX_LINE_BYTES + 1]).is_err());
        let mut decoder = LineDecoder::default();
        assert!(decoder.push(&[0xff, b'\n']).is_err());
    }
}
