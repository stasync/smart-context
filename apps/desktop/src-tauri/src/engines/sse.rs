//! A minimal Server-Sent Events parser: bytes in, (event, data) pairs out.
//! Chunks may split lines and events anywhere.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

#[derive(Debug, Default)]
pub struct SseParser {
    buffer: Vec<u8>,
    event: String,
    data: Vec<String>,
}

impl SseParser {
    /// Feeds a chunk and returns the events it completed.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(end) = self.buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line[..line.len() - 1]);
            let line = line.strip_suffix('\r').unwrap_or(&line);
            if line.is_empty() {
                if let Some(event) = self.dispatch() {
                    events.push(event);
                }
                continue;
            }
            if line.starts_with(':') {
                continue; // a comment
            }
            let (field, value) = match line.split_once(':') {
                Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
                None => (line, ""),
            };
            match field {
                "event" => self.event = value.to_string(),
                "data" => self.data.push(value.to_string()),
                _ => {} // id, retry: unused
            }
        }
        events
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        let event = std::mem::take(&mut self.event);
        let data = std::mem::take(&mut self.data);
        if data.is_empty() {
            return None;
        }
        Some(SseEvent {
            event: if event.is_empty() {
                "message".into()
            } else {
                event
            },
            data: data.join("\n"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STREAM: &str = "event: message_start\ndata: {\"a\":1}\n\n: keep-alive\n\nevent: ping\r\ndata: {}\r\n\r\ndata: line one\ndata: line two\n\n";

    fn all_at_once() -> Vec<SseEvent> {
        SseParser::default().push(STREAM.as_bytes())
    }

    #[test]
    fn parses_events_comments_and_multiline_data() {
        let events = all_at_once();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event, "message_start");
        assert_eq!(events[0].data, "{\"a\":1}");
        assert_eq!(events[1].event, "ping");
        assert_eq!(events[2].event, "message");
        assert_eq!(events[2].data, "line one\nline two");
    }

    #[test]
    fn any_chunking_gives_the_same_events() {
        for size in [1, 2, 3, 7, 13] {
            let mut parser = SseParser::default();
            let events: Vec<SseEvent> = STREAM
                .as_bytes()
                .chunks(size)
                .flat_map(|c| parser.push(c))
                .collect();
            assert_eq!(events, all_at_once(), "chunk size {size}");
        }
    }

    #[test]
    fn multibyte_characters_survive_split_chunks() {
        let text = "event: x\ndata: café ☕\n\n".as_bytes();
        let mut parser = SseParser::default();
        let events: Vec<SseEvent> = text.chunks(1).flat_map(|c| parser.push(c)).collect();
        assert_eq!(events[0].data, "café ☕");
    }
}
