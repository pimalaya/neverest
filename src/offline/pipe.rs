//! # Bounded in-memory pipe
//!
//! A [`Write`] end feeding a [`Read`] end across threads, blocking the writer
//! when the buffer is full and the reader when it is empty, so a body streams
//! from one connection to another without ever being held whole in memory.
//!
//! Used by relay, a two-source pass-through copy: one thread streams a fetch
//! into the writer while another streams its output into an append.

use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    sync::{Arc, Condvar, Mutex},
};

/// The pipe's bounded byte buffer plus its closed flag.
struct Shared {
    buf: Mutex<Buffer>,
    /// Signalled when bytes are added or the writer closes.
    readable: Condvar,
    /// Signalled when bytes are consumed.
    writable: Condvar,
    capacity: usize,
}

struct Buffer {
    data: VecDeque<u8>,
    closed: bool,
}

/// The write end; dropping it closes the pipe (EOF once the reader drains).
pub struct PipeWriter {
    shared: Arc<Shared>,
}

/// The read end.
pub struct PipeReader {
    shared: Arc<Shared>,
}

/// Creates a bounded pipe holding at most `capacity` buffered bytes.
pub fn bounded(capacity: usize) -> (PipeWriter, PipeReader) {
    let shared = Arc::new(Shared {
        buf: Mutex::new(Buffer {
            data: VecDeque::new(),
            closed: false,
        }),
        readable: Condvar::new(),
        writable: Condvar::new(),
        capacity: capacity.max(1),
    });
    (
        PipeWriter {
            shared: shared.clone(),
        },
        PipeReader { shared },
    )
}

impl Write for PipeWriter {
    fn write(&mut self, mut bytes: &[u8]) -> io::Result<usize> {
        let total = bytes.len();
        let mut guard = self.shared.buf.lock().unwrap();
        while !bytes.is_empty() {
            while guard.data.len() >= self.shared.capacity {
                guard = self.shared.writable.wait(guard).unwrap();
            }
            let room = self.shared.capacity - guard.data.len();
            let take = room.min(bytes.len());
            guard.data.extend(&bytes[..take]);
            bytes = &bytes[take..];
            self.shared.readable.notify_one();
        }
        Ok(total)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for PipeWriter {
    fn drop(&mut self) {
        let mut guard = self.shared.buf.lock().unwrap();
        guard.closed = true;
        self.shared.readable.notify_all();
    }
}

impl Read for PipeReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let mut guard = self.shared.buf.lock().unwrap();
        loop {
            if let Some(&first) = guard.data.front() {
                out[0] = first;
                guard.data.pop_front();
                let mut n = 1;
                while n < out.len() {
                    match guard.data.pop_front() {
                        Some(b) => {
                            out[n] = b;
                            n += 1;
                        }
                        None => break,
                    }
                }
                self.shared.writable.notify_one();
                return Ok(n);
            }
            if guard.closed {
                return Ok(0);
            }
            guard = self.shared.readable.wait(guard).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    fn streams_across_threads_bounded() {
        let body: Vec<u8> = (0..100_000u32).map(|i| i as u8).collect();
        let (mut w, mut r) = bounded(4096);

        let src = body.clone();
        let producer = thread::spawn(move || {
            w.write_all(&src).unwrap();
        });

        let mut got = Vec::new();
        r.read_to_end(&mut got).unwrap();
        producer.join().unwrap();

        assert_eq!(got, body);
    }

    #[test]
    fn reader_sees_eof_after_writer_closes() {
        let (w, mut r) = bounded(16);
        drop(w);
        let mut got = Vec::new();
        assert_eq!(r.read_to_end(&mut got).unwrap(), 0);
    }
}
