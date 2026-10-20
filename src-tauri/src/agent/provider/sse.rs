//! Shared incremental SSE framing over a byte stream.
//!
//! Providers feed raw HTTP body chunks in with [`SseBuffer::push_chunk`] and
//! pull complete (newline-terminated, CR-stripped, non-empty) lines out with
//! [`SseBuffer::next_line`]. Event dispatch stays provider-specific: both the
//! OpenAI and Anthropic streaming APIs embed the event type in the JSON
//! `data:` payload, so only the payload needs to be extracted here.

/// Incremental line buffer for an SSE body.
#[derive(Default)]
pub struct SseBuffer {
    buffer: String,
}

impl SseBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a raw body chunk. Invalid UTF-8 is replaced lossily; SSE bodies
    /// are JSON text so this only matters for a corrupted stream.
    pub fn push_chunk(&mut self, bytes: &[u8]) {
        self.buffer.push_str(&String::from_utf8_lossy(bytes));
    }

    /// Return the next complete non-empty line, or `None` if the buffer holds
    /// no full line yet. `\r\n` line endings are normalized; blank separator
    /// lines between events are skipped.
    pub fn next_line(&mut self) -> Option<String> {
        loop {
            let pos = self.buffer.find('\n')?;
            let mut line: String = self.buffer.drain(..=pos).collect();
            if line.ends_with('\n') {
                line.pop();
            }
            if line.ends_with('\r') {
                line.pop();
            }
            if line.is_empty() {
                continue;
            }
            return Some(line);
        }
    }
}

/// Extract the payload of an SSE `data:` line, if this is one.
/// `data:[DONE]` and `data: {...}` are both handled; the leading space after
/// the colon is optional per the SSE spec.
pub fn data_payload(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("data:")?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_lines_across_chunks() {
        let mut buf = SseBuffer::new();
        buf.push_chunk(b"dat");
        assert!(buf.next_line().is_none());
        buf.push_chunk(b"a: {\"a\":1}\ndata: {\"b\":2}\r\n");
        assert_eq!(buf.next_line().unwrap(), "data: {\"a\":1}");
        assert_eq!(buf.next_line().unwrap(), "data: {\"b\":2}");
        assert!(buf.next_line().is_none());
    }

    #[test]
    fn skips_blank_separator_lines() {
        let mut buf = SseBuffer::new();
        buf.push_chunk(b"\n\ndata: hi\n");
        assert_eq!(buf.next_line().unwrap(), "data: hi");
    }

    #[test]
    fn data_payload_variants() {
        assert_eq!(data_payload("data: [DONE]"), Some("[DONE]"));
        assert_eq!(data_payload("data:{\"x\":1}"), Some("{\"x\":1}"));
        assert_eq!(data_payload("event: ping"), None);
        assert_eq!(data_payload(": comment"), None);
    }
}
