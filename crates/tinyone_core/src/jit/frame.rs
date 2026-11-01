//! Execution-local call state and bounded reuse of Ralloc-owned frame memory.

use super::call_feedback::{CallSample, GuardedCall};
use crate::runtime::value_codec::ENCODED_VALUE_BYTES;
use crate::{JitStats, Result, TinyMemory, Value};

const MAX_IDLE_FRAMES: usize = 8;
const MAX_IDLE_FRAME_BYTES: usize = 64 * 1024;

pub(super) struct FrameCursor {
    pub(super) chunk_index: usize,
    pub(super) pc:          usize,
    // Values are pushed/popped and may own Strings. Vec tracks the initialized
    // prefix and drops removed values; a boxed capacity buffer would need a
    // second length, Option-filled slots, or unsafe initialization tracking.
    pub(super) stack:       Vec<Value>,
}

/// Owns handles, not references into the call vector. Relocating this header
/// does not move its Ralloc payload or operand-stack allocation.
pub(super) struct CallFrame {
    pub(super) cursor:     FrameCursor,
    pub(super) memory:     TinyMemory,
    pub(super) slot_count: usize,
}

#[derive(Clone, Copy)]
pub(super) enum CallRequest {
    Function {
        index:     usize,
        arg_count: usize,
    },
    /// The callable precedes the arguments on the suspended operand stack.
    Operand {
        arg_count: usize,
    },
    Sampled {
        arg_count: usize,
        sample:    CallSample,
    },
    Guarded(GuardedCall),
    UnseenOperand {
        arg_count: usize,
    },
}

impl CallRequest {
    pub(super) fn arg_count(self) -> usize {
        match self {
            Self::Function { arg_count, .. }
            | Self::Operand { arg_count }
            | Self::Sampled { arg_count, .. }
            | Self::UnseenOperand { arg_count } => arg_count,
            Self::Guarded(guard) => guard.arg_count(),
        }
    }
}

/// Calls suspend a cursor; returns resume its caller. Neither event recurses
/// through the host interpreter. The caller retains its arguments until frame
/// entry drains them directly into the callee's slots.
pub(super) enum FrameExit {
    Call(CallRequest),
    Return(Option<Value>),
}

struct IdleFrame {
    slot_count: usize,
    memory:     TinyMemory,
}

#[derive(Default)]
pub(super) struct FrameMemoryPool {
    // The live pool grows/shrinks, never exceeds MAX_IDLE_FRAMES, and remains
    // allocation-free when unused. Do not box each frame: those are already
    // owning handles to separate Ralloc allocations.
    frames:         Vec<IdleFrame>,
    retained_bytes: usize,
}

impl FrameMemoryPool {
    pub(super) fn take(&mut self, slot_count: usize, stats: &mut JitStats) -> Result<TinyMemory> {
        if let Some(index) = self.frames.iter().position(|frame| frame.slot_count == slot_count) {
            let frame = self.frames.swap_remove(index);
            self.retained_bytes -= frame.slot_count * ENCODED_VALUE_BYTES;
            stats.frame_memory_reuses = stats.frame_memory_reuses.saturating_add(1);
            // recycle() cleared every encoded slot before admitting it.
            return Ok(frame.memory);
        }
        let memory = match TinyMemory::try_new(slot_count) {
            Ok(memory) => memory,
            Err(error) if self.frames.is_empty() => return Err(error),
            Err(_) => {
                // Reuse must not turn an otherwise satisfiable frame request
                // into allocator exhaustion merely by retaining idle storage.
                self.clear();
                TinyMemory::try_new(slot_count)?
            }
        };
        if slot_count != 0 {
            stats.frame_memory_allocations = stats.frame_memory_allocations.saturating_add(1);
        }
        Ok(memory)
    }

    pub(super) fn recycle(&mut self, mut memory: TinyMemory, slot_count: usize, stats: &mut JitStats) {
        // slot_count came from a verified function (at most 65,536 slots).
        let byte_len = slot_count * ENCODED_VALUE_BYTES;
        if byte_len == 0
            || self.frames.len() >= MAX_IDLE_FRAMES
            || byte_len > MAX_IDLE_FRAME_BYTES.saturating_sub(self.retained_bytes)
        {
            return;
        }
        memory.reset();
        self.frames.push(IdleFrame { slot_count, memory });
        self.retained_bytes += byte_len;
        stats.peak_idle_frame_bytes = stats.peak_idle_frame_bytes.max(self.retained_bytes);
    }

    /// Release idle storage before runtime heap allocation/growth, so reuse
    /// cannot steal arena capacity from observable program allocations.
    pub(super) fn clear(&mut self) {
        self.frames.clear();
        self.retained_bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "frame-storage diagnostic; run explicitly with --ignored --nocapture"]
    fn report_frame_container_layout() {
        eprintln!(
            "frame layout: CallFrame={} Option<CallFrame>={} IdleFrame={} Option<IdleFrame>={} CallRequest={} FrameExit={}",
            size_of::<CallFrame>(),
            size_of::<Option<CallFrame>>(),
            size_of::<IdleFrame>(),
            size_of::<Option<IdleFrame>>(),
            size_of::<CallRequest>(),
            size_of::<FrameExit>(),
        );
        for depth in [0, 1, 4, 8, 16] {
            let mut frames = Vec::new();
            let mut capacity_changes = 0;
            for _ in 0..depth {
                let before = frames.capacity();
                frames.push(CallFrame {
                    cursor:     FrameCursor {
                        chunk_index: 1,
                        pc:          0,
                        stack:       Vec::new(),
                    },
                    memory:     TinyMemory::default(),
                    slot_count: 0,
                });
                capacity_changes += usize::from(frames.capacity() != before);
            }
            eprintln!(
                "depth={depth}: vec_reserved_bytes={} capacity_changes={capacity_changes} boxed_maximum_bytes={}",
                frames.capacity() * size_of::<CallFrame>(),
                crate::runtime::limits::MAX_CALL_DEPTH * size_of::<Option<CallFrame>>(),
            );
        }
    }

    #[test]
    fn idle_frames_have_independent_count_and_byte_bounds() {
        let mut pool = FrameMemoryPool::default();
        let mut stats = JitStats::default();
        for _ in 0..12 {
            pool.recycle(TinyMemory::try_new(1).expect("small frame"), 1, &mut stats);
        }
        assert_eq!(pool.frames.len(), MAX_IDLE_FRAMES);
        pool.clear();
        for _ in 0..12 {
            pool.recycle(TinyMemory::try_new(256).expect("medium frame"), 256, &mut stats);
        }
        assert_eq!(pool.frames.len(), 4);
        assert_eq!(pool.retained_bytes, MAX_IDLE_FRAME_BYTES);
        assert_eq!(stats.peak_idle_frame_bytes, MAX_IDLE_FRAME_BYTES);
        pool.clear();
        assert_eq!(pool.retained_bytes, 0);
    }
}
