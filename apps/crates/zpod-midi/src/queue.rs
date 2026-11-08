//! Bounded single-producer / single-consumer queue. The buffer is allocated
//! once; push and pop never allocate, lock or block. A full queue drops the
//! message, counts it and raises `lost`, so the consumer can clean up
//! (note-offs may have been among the dropped messages).

use crate::encode::Msg;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    /// due time, ns since the output's epoch
    pub due_ns: u64,
    pub msg: Msg,
}

pub struct Ring {
    buf: Box<[UnsafeCell<Item>]>,
    head: AtomicUsize, // next to pop (consumer)
    tail: AtomicUsize, // next to push (producer)
    pub dropped: AtomicU64,
    pub lost: AtomicBool,
}

// SAFETY: one producer writes slots in [tail], one consumer reads slots in
// [head]; a slot is published with Release on tail and taken with Acquire.
unsafe impl Sync for Ring {}
unsafe impl Send for Ring {}

impl Ring {
    pub fn new(cap: usize) -> Ring {
        let cap = cap.max(2).next_power_of_two();
        let buf = (0..cap).map(|_| UnsafeCell::new(Item { due_ns: 0, msg: Msg::EMPTY })).collect();
        Ring { buf, head: AtomicUsize::new(0), tail: AtomicUsize::new(0), dropped: AtomicU64::new(0), lost: AtomicBool::new(false) }
    }
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }
    pub fn len(&self) -> usize {
        self.tail.load(Ordering::Acquire).wrapping_sub(self.head.load(Ordering::Acquire))
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Producer only.
    pub fn push(&self, it: Item) -> bool {
        let t = self.tail.load(Ordering::Relaxed);
        let h = self.head.load(Ordering::Acquire);
        if t.wrapping_sub(h) >= self.buf.len() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            self.lost.store(true, Ordering::Release);
            return false;
        }
        // SAFETY: slot t is not visible to the consumer until tail moves
        unsafe { *self.buf[t & (self.buf.len() - 1)].get() = it };
        self.tail.store(t.wrapping_add(1), Ordering::Release);
        true
    }
    /// Consumer only.
    pub fn pop(&self) -> Option<Item> {
        let h = self.head.load(Ordering::Relaxed);
        if h == self.tail.load(Ordering::Acquire) {
            return None;
        }
        // SAFETY: slot h was published by the producer and is not reused
        // until head moves past it
        let it = unsafe { *self.buf[h & (self.buf.len() - 1)].get() };
        self.head.store(h.wrapping_add(1), Ordering::Release);
        Some(it)
    }
}
