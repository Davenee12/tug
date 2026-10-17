//! The microphone, for one hold: Windows' wave-in API on the default recording device, asked for
//! 16 kHz mono 16-bit (Windows converts from whatever the device does). Samples go into memory
//! only, and the device is closed the moment the hold ends (or this is dropped).
//!
//! Polled rather than called back: the hold loop already wakes every few milliseconds to check
//! the keys, so it collects finished buffers then. Eight 100 ms buffers give 800 ms of slack.

use windows::Win32::Media::Audio::{
    waveInAddBuffer, waveInClose, waveInGetNumDevs, waveInOpen, waveInPrepareHeader, waveInReset, waveInStart,
    waveInStop, waveInUnprepareHeader, CALLBACK_NULL, HWAVEIN, WAVEHDR, WAVE_MAPPER, WHDR_DONE,
};

use super::sapi::wave_format;

const BUFFERS: usize = 8;
/// 100 ms of 16 kHz 16-bit mono.
const BUFFER_BYTES: usize = 3200;
/// MMSYSERR_ALLOCATED, MMSYSERR_BADDEVICEID, MMSYSERR_NODRIVER, WAVERR_BADFORMAT: no usable device.
const NO_DEVICE: [u32; 4] = [4, 2, 6, 32];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    NoDevice,
    Failed(u32),
}

/// Whether Windows has any recording device at all.
pub fn has_device() -> bool {
    // SAFETY: a read-only query.
    unsafe { waveInGetNumDevs() > 0 }
}

struct Buffer {
    header: WAVEHDR,
    data: Vec<u8>,
}

pub struct Recording {
    handle: HWAVEIN,
    /// Filled once before any is handed to Windows and never resized, so the headers Windows
    /// holds pointers to never move.
    buffers: Vec<Buffer>,
    samples: Vec<i16>,
    /// The oldest buffer still queued.
    next: usize,
    open: bool,
}

// SAFETY: the handle and buffers are only touched by the thread holding the Recording; wave-in
// handles aren't tied to the thread that opened them.
unsafe impl Send for Recording {}

impl Recording {
    /// Open the default microphone and start recording.
    pub fn start() -> Result<Self, OpenError> {
        let format = wave_format();
        let mut handle = HWAVEIN::default();
        // SAFETY: valid out-pointer and format; no callback (polled).
        let rc = unsafe { waveInOpen(Some(&mut handle), WAVE_MAPPER, &format, None, None, CALLBACK_NULL) };
        if rc != 0 {
            return Err(if NO_DEVICE.contains(&rc) {
                OpenError::NoDevice
            } else {
                OpenError::Failed(rc)
            });
        }
        let buffers = (0..BUFFERS)
            .map(|_| Buffer {
                header: WAVEHDR::default(),
                data: vec![0u8; BUFFER_BYTES],
            })
            .collect();
        let mut rec = Self {
            handle,
            buffers,
            samples: Vec::new(),
            next: 0,
            open: true,
        };
        let size = std::mem::size_of::<WAVEHDR>() as u32;
        let mut queued = true;
        for b in &mut rec.buffers {
            b.header.lpData = windows::core::PSTR(b.data.as_mut_ptr());
            b.header.dwBufferLength = BUFFER_BYTES as u32;
            // SAFETY: the header and its data live in `buffers`, which is never resized, until
            // `close` unprepares them.
            let ok = unsafe {
                waveInPrepareHeader(handle, &mut b.header, size) == 0
                    && waveInAddBuffer(handle, &mut b.header, size) == 0
            };
            if !ok {
                queued = false;
                break;
            }
        }
        if !queued {
            rec.close();
            return Err(OpenError::Failed(0));
        }
        // SAFETY: an open handle with buffers queued.
        let rc = unsafe { waveInStart(rec.handle) };
        if rc != 0 {
            rec.close();
            return Err(OpenError::Failed(rc));
        }
        Ok(rec)
    }

    /// Collect finished buffers, oldest first, and hand them back to Windows. Windows fills them
    /// in the order they were queued, so they're walked as a ring from the oldest.
    pub fn poll(&mut self) {
        let size = std::mem::size_of::<WAVEHDR>() as u32;
        for _ in 0..self.buffers.len() {
            let b = &mut self.buffers[self.next];
            if b.header.dwFlags & WHDR_DONE == 0 {
                break;
            }
            self.next = (self.next + 1) % BUFFERS;
            let n = (b.header.dwBytesRecorded as usize).min(b.data.len());
            self.samples
                .extend(b.data[..n].as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)));
            b.header.dwBytesRecorded = 0;
            b.header.dwFlags &= !WHDR_DONE;
            if self.open {
                // SAFETY: a prepared header of ours, done with by Windows.
                unsafe {
                    let _ = waveInAddBuffer(self.handle, &mut b.header, size);
                }
            }
        }
    }

    /// Stop, close the device, and return everything recorded.
    pub fn finish(mut self) -> Vec<i16> {
        self.close();
        std::mem::take(&mut self.samples)
    }

    fn close(&mut self) {
        if !self.open {
            return;
        }
        let size = std::mem::size_of::<WAVEHDR>() as u32;
        // SAFETY: our open handle; reset marks every queued buffer done, so they can be collected
        // and unprepared before closing.
        unsafe {
            let _ = waveInStop(self.handle);
            let _ = waveInReset(self.handle);
        }
        self.open = false;
        self.poll();
        for b in &mut self.buffers {
            // SAFETY: as above.
            unsafe {
                let _ = waveInUnprepareHeader(self.handle, &mut b.header, size);
            }
        }
        // SAFETY: as above; nothing is queued any more.
        unsafe {
            let _ = waveInClose(self.handle);
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.close();
    }
}
