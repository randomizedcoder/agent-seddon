//! The rolling output buffer.
//!
//! This is the leak-critical path. A `dev-server` or a stray `yes` produces
//! output faster than anyone reads it, so retention is **bounded** and the
//! oldest bytes are dropped — but the cursor space stays **absolute**, so a
//! reader that falls behind learns exactly how much it lost instead of silently
//! receiving a gap.

/// Retained bytes per session. Past this, the oldest are dropped.
pub const BUFFER_LIMIT: usize = 2 * 1024 * 1024;

/// A bounded FIFO of output bytes with absolute cursor positions.
pub struct RollingBuffer {
    buf: std::collections::VecDeque<u8>,
    /// Absolute offset of `buf[0]`.
    first: u64,
    /// Absolute offset just past the last byte pushed.
    next: u64,
    limit: usize,
}

impl RollingBuffer {
    pub fn new(limit: usize) -> Self {
        Self {
            buf: std::collections::VecDeque::new(),
            first: 0,
            next: 0,
            limit: limit.max(1),
        }
    }

    pub fn first_retained(&self) -> u64 {
        self.first
    }
    pub fn next_cursor(&self) -> u64 {
        self.next
    }
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend(data.iter().copied());
        self.next = self.next.saturating_add(data.len() as u64);
        // Drop from the front until within the cap, advancing `first` so the
        // absolute cursor space stays consistent.
        while self.buf.len() > self.limit {
            let overflow = self.buf.len() - self.limit;
            self.buf.drain(0..overflow);
            self.first = self.first.saturating_add(overflow as u64);
        }
    }

    /// Read from an absolute `cursor` (`None` ⇒ the oldest retained byte).
    ///
    /// Returns `(data, next_cursor, dropped)`, where `dropped` is how many bytes
    /// the caller missed because they had already been evicted.
    pub fn read_from(&self, cursor: Option<u64>) -> (Vec<u8>, u64, u64) {
        let want = cursor.unwrap_or(self.first);
        // A cursor ahead of what we have (a caller replaying a stale handle
        // against a restarted session) yields nothing rather than panicking.
        if want >= self.next {
            return (Vec::new(), self.next, 0);
        }
        let (start, dropped) = if want < self.first {
            (self.first, self.first - want)
        } else {
            (want, 0)
        };
        let offset = (start - self.first) as usize;
        let data: Vec<u8> = self.buf.iter().skip(offset).copied().collect();
        (data, self.next, dropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn positive_reads_everything_from_the_start() {
        let mut b = RollingBuffer::new(1024);
        b.push(b"hello ");
        b.push(b"world");
        let (data, next, dropped) = b.read_from(None);
        assert_eq!(data, b"hello world");
        assert_eq!(next, 11);
        assert_eq!(dropped, 0);
    }

    /// Resuming from a cursor returns only what is new — the property the
    /// streaming read depends on.
    #[test]
    fn positive_resume_from_cursor_returns_only_new_bytes() {
        let mut b = RollingBuffer::new(1024);
        b.push(b"abc");
        let (_, cursor, _) = b.read_from(None);
        b.push(b"def");
        let (data, next, dropped) = b.read_from(Some(cursor));
        assert_eq!(data, b"def");
        assert_eq!(next, 6);
        assert_eq!(dropped, 0);
    }

    /// A firehose must not grow memory without bound.
    #[test]
    fn adversarial_output_is_bounded() {
        let mut b = RollingBuffer::new(100);
        for _ in 0..1_000 {
            b.push(&[b'x'; 50]);
        }
        assert!(b.len() <= 100, "buffer grew to {}", b.len());
        assert_eq!(b.next_cursor(), 50_000, "absolute cursor keeps counting");
    }

    /// A reader that fell behind must be TOLD how much it lost, not handed a
    /// silent gap.
    #[test]
    fn positive_dropped_bytes_are_reported() {
        let mut b = RollingBuffer::new(10);
        b.push(b"0123456789");
        b.push(b"abcdefghij"); // evicts the first ten
        let (data, _next, dropped) = b.read_from(Some(0));
        assert_eq!(dropped, 10, "the caller must learn it missed 10 bytes");
        assert_eq!(data, b"abcdefghij");
        assert_eq!(b.first_retained(), 10);
    }

    /// Cursors from a stale handle must not panic or return nonsense.
    #[rstest]
    #[case::adversarial_cursor_far_ahead(9_999_999)]
    #[case::adversarial_cursor_at_end(6)]
    #[case::boundary_cursor_zero(0)]
    fn adversarial_out_of_range_cursor_is_safe(#[case] cursor: u64) {
        let mut b = RollingBuffer::new(1024);
        b.push(b"abcdef");
        let (data, next, _dropped) = b.read_from(Some(cursor));
        assert_eq!(next, 6);
        assert!(data.len() <= 6);
    }

    #[test]
    fn boundary_empty_buffer_reads_nothing() {
        let b = RollingBuffer::new(1024);
        let (data, next, dropped) = b.read_from(None);
        assert!(data.is_empty());
        assert_eq!((next, dropped), (0, 0));
        assert!(b.is_empty());
    }

    /// A single push larger than the whole limit must still leave the buffer
    /// bounded and the cursor space coherent.
    #[test]
    fn adversarial_single_push_over_the_limit() {
        let mut b = RollingBuffer::new(10);
        b.push(&[b'z'; 1_000]);
        assert_eq!(b.len(), 10);
        assert_eq!(b.next_cursor(), 1_000);
        assert_eq!(b.first_retained(), 990);
        let (data, _, dropped) = b.read_from(Some(0));
        assert_eq!(data.len(), 10);
        assert_eq!(dropped, 990);
    }
}
