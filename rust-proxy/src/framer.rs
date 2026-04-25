// NDJSON message framer. Mirrors the Java MessageFramer behavior:
// - delimiter is 0x0A (LF), trailing \r is stripped
// - if a single message exceeds MAX_BUFFER_SIZE, drop bytes silently
//   until the next LF and resync
// - returns whole messages as Vec<u8>; partial messages remain in the
//   buffer between feed() calls

const MAX_BUFFER_SIZE: usize = 10 * 1024 * 1024; // 10 MiB

pub struct Framer {
    buffer: Vec<u8>,
    discarding: bool,
}

impl Framer {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(4096),
            discarding: false,
        }
    }

    /// Feed bytes; return any complete messages produced.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut out: Vec<Vec<u8>> = Vec::new();

        for &b in bytes {
            if !self.discarding && self.buffer.len() > MAX_BUFFER_SIZE {
                self.discarding = true;
                self.buffer.clear();
                eprintln!(
                    "[fluxmirror-proxy] WARN message exceeded {MAX_BUFFER_SIZE} bytes, discarding until next newline"
                );
            }

            if self.discarding {
                if b == 0x0A {
                    self.discarding = false;
                }
                continue;
            }

            if b == 0x0A {
                let mut msg = std::mem::take(&mut self.buffer);
                // Strip trailing \r if present.
                if msg.last() == Some(&0x0D) {
                    msg.pop();
                }
                out.push(msg);
                self.buffer = Vec::with_capacity(4096);
            } else {
                self.buffer.push(b);
            }
        }
        out
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.discarding = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line() {
        let mut f = Framer::new();
        let msgs = f.feed(b"hello\n");
        assert_eq!(msgs, vec![b"hello".to_vec()]);
    }

    #[test]
    fn strips_carriage_return() {
        let mut f = Framer::new();
        let msgs = f.feed(b"hello\r\n");
        assert_eq!(msgs, vec![b"hello".to_vec()]);
    }

    #[test]
    fn split_across_feeds() {
        let mut f = Framer::new();
        assert!(f.feed(b"par").is_empty());
        assert!(f.feed(b"tial").is_empty());
        let msgs = f.feed(b"\nrest");
        assert_eq!(msgs, vec![b"partial".to_vec()]);
        let msgs = f.feed(b" of line\n");
        assert_eq!(msgs, vec![b"rest of line".to_vec()]);
    }

    #[test]
    fn multiple_in_one_chunk() {
        let mut f = Framer::new();
        let msgs = f.feed(b"a\nb\nc\n");
        assert_eq!(
            msgs,
            vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]
        );
    }

    #[test]
    fn empty_line_yields_empty_message() {
        let mut f = Framer::new();
        let msgs = f.feed(b"\n");
        assert_eq!(msgs, vec![Vec::<u8>::new()]);
    }

    #[test]
    fn oversize_message_discarded_then_resync() {
        let mut f = Framer::new();
        // Feed 10 MiB + 1 of garbage with no newline → triggers discard
        let huge = vec![b'x'; MAX_BUFFER_SIZE + 1];
        let m1 = f.feed(&huge);
        assert!(m1.is_empty());
        // feeding more without LF → still discarding
        let m2 = f.feed(b"more garbage");
        assert!(m2.is_empty());
        // newline ends discard
        let m3 = f.feed(b"\nrecovered\n");
        assert_eq!(m3, vec![b"recovered".to_vec()]);
    }
}
