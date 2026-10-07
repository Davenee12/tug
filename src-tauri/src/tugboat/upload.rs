//! Resume bookkeeping for a file the phone is sending: how it's cut into chunks, which chunks
//! have arrived, and the compact "received" ranges the page asks for after Safari was suspended.

/// Largest file the phone may send (per file).
pub const MAX_FILE: u64 = 8 * 1024 * 1024 * 1024;
pub const MIN_CHUNK: u32 = 64 * 1024;
pub const MAX_CHUNK: u32 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanError {
    TooBig,
    BadChunkSize,
}

/// How a file of `size` bytes is cut into `chunk_size` pieces. A zero-byte file is one empty
/// chunk, so every file has at least one chunk to send and finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub size: u64,
    pub chunk_size: u32,
}

impl Plan {
    pub fn new(size: u64, chunk_size: u32) -> Result<Plan, PlanError> {
        if size > MAX_FILE {
            return Err(PlanError::TooBig);
        }
        if !(MIN_CHUNK..=MAX_CHUNK).contains(&chunk_size) {
            return Err(PlanError::BadChunkSize);
        }
        Ok(Plan { size, chunk_size })
    }

    pub fn chunks(&self) -> u32 {
        // MAX_FILE / MIN_CHUNK = 131072, so this always fits in u32.
        (self.size.div_ceil(self.chunk_size as u64)).max(1) as u32
    }

    pub fn offset(&self, index: u32) -> u64 {
        index as u64 * self.chunk_size as u64
    }

    /// Plaintext length chunk `index` must have, or `None` if there's no such chunk.
    pub fn expected_len(&self, index: u32) -> Option<usize> {
        if index >= self.chunks() {
            return None;
        }
        let start = self.offset(index);
        let end = (start + self.chunk_size as u64).min(self.size);
        Some((end - start) as usize)
    }
}

/// Which chunks have arrived.
#[derive(Debug, Clone)]
pub struct Received {
    bits: Vec<u64>,
    total: u32,
    count: u32,
}

impl Received {
    pub fn new(total: u32) -> Received {
        Received {
            bits: vec![0; total.div_ceil(64) as usize],
            total,
            count: 0,
        }
    }

    pub fn has(&self, i: u32) -> bool {
        i < self.total && self.bits[(i / 64) as usize] & (1 << (i % 64)) != 0
    }

    /// Mark chunk `i` as written; false if it was already (a retried chunk) or out of range.
    pub fn mark(&mut self, i: u32) -> bool {
        if i >= self.total || self.has(i) {
            return false;
        }
        self.bits[(i / 64) as usize] |= 1 << (i % 64);
        self.count += 1;
        true
    }

    #[cfg(test)]
    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn complete(&self) -> bool {
        self.count == self.total
    }

    /// Bytes received so far under `plan` (the session keeps a running total instead).
    #[cfg(test)]
    pub fn bytes(&self, plan: &Plan) -> u64 {
        (0..self.total)
            .filter(|&i| self.has(i))
            .map(|i| plan.expected_len(i).unwrap_or(0) as u64)
            .sum()
    }

    /// Received chunks as half-open `[start, end)` ranges, which stay short for the usual
    /// "everything up to where Safari paused" shape.
    pub fn ranges(&self) -> Vec<[u32; 2]> {
        let mut out: Vec<[u32; 2]> = Vec::new();
        for i in (0..self.total).filter(|&i| self.has(i)) {
            match out.last_mut() {
                Some(last) if last[1] == i => last[1] = i + 1,
                _ => out.push([i, i + 1]),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u32 = 1024 * 1024;

    #[test]
    fn plans_chunks() {
        let p = Plan::new(5 * MB as u64 + 10, 2 * MB).unwrap();
        assert_eq!(p.chunks(), 3);
        assert_eq!(p.expected_len(0), Some(2 * MB as usize));
        assert_eq!(p.expected_len(2), Some(MB as usize + 10));
        assert_eq!(p.expected_len(3), None);
        assert_eq!(p.offset(2), 4 * MB as u64);
        // Exactly on a boundary: no empty trailing chunk.
        assert_eq!(Plan::new(4 * MB as u64, 2 * MB).unwrap().chunks(), 2);
        // An empty file is one empty chunk.
        let empty = Plan::new(0, 2 * MB).unwrap();
        assert_eq!(empty.chunks(), 1);
        assert_eq!(empty.expected_len(0), Some(0));
    }

    #[test]
    fn enforces_limits() {
        assert_eq!(Plan::new(MAX_FILE + 1, 2 * MB), Err(PlanError::TooBig));
        assert!(Plan::new(MAX_FILE, 2 * MB).is_ok());
        assert_eq!(Plan::new(10, 1024), Err(PlanError::BadChunkSize));
        assert_eq!(Plan::new(10, 64 * MB), Err(PlanError::BadChunkSize));
        // The smallest chunk on the largest file still fits the chunk counter.
        assert_eq!(Plan::new(MAX_FILE, MIN_CHUNK).unwrap().chunks(), 131_072);
    }

    #[test]
    fn tracks_received_chunks_and_ranges() {
        let plan = Plan::new(10 * MB as u64 - 1, MB).unwrap();
        let mut r = Received::new(plan.chunks());
        assert_eq!(r.ranges(), Vec::<[u32; 2]>::new());
        for i in [0, 1, 2, 5, 9] {
            assert!(r.mark(i));
        }
        assert!(!r.mark(2), "a retried chunk counts once");
        assert!(!r.mark(10), "out of range");
        assert_eq!(r.count(), 5);
        assert_eq!(r.ranges(), vec![[0, 3], [5, 6], [9, 10]]);
        assert_eq!(r.bytes(&plan), 4 * MB as u64 + (MB as u64 - 1));
        assert!(!r.complete());
        for i in 0..10 {
            r.mark(i);
        }
        assert!(r.complete());
        assert_eq!(r.ranges(), vec![[0, 10]]);
        assert_eq!(r.bytes(&plan), plan.size);
    }

    #[test]
    fn handles_more_than_64_chunks() {
        let mut r = Received::new(130);
        assert!(r.mark(64));
        assert!(r.mark(129));
        assert!(r.has(64) && r.has(129) && !r.has(63));
        assert_eq!(r.ranges(), vec![[64, 65], [129, 130]]);
    }
}
