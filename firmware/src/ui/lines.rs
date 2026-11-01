use crate::bounce::{HEIGHT, WIDTH};
use slint::platform::software_renderer::{LineBufferProvider, Rgb565Pixel};

/// Blend in internal RAM, then copy the finished span to PSRAM.
pub(super) struct PartialLines {
    fb: *mut Rgb565Pixel,
    line: [Rgb565Pixel; WIDTH],
    flipped: bool,
}

impl PartialLines {
    pub(super) fn new(fb: *mut Rgb565Pixel, flipped: bool) -> Self {
        Self {
            fb,
            line: [Rgb565Pixel(0); WIDTH],
            flipped,
        }
    }
}

impl LineBufferProvider for PartialLines {
    type TargetPixel = Rgb565Pixel;

    fn process_line(
        &mut self,
        line: usize,
        range: core::ops::Range<usize>,
        render_fn: impl FnOnce(&mut [Rgb565Pixel]),
    ) {
        assert!(range.start <= range.end && range.end <= WIDTH && line < HEIGHT);
        let len = range.len();
        let (line, start) = if self.flipped {
            (HEIGHT - 1 - line, WIDTH - range.end)
        } else {
            (line, range.start)
        };
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.fb.add(line * WIDTH + start),
                self.line.as_mut_ptr(),
                len,
            );
        }
        let pixels = &mut self.line[..len];
        if self.flipped {
            pixels.reverse();
        }
        render_fn(pixels);
        if self.flipped {
            pixels.reverse();
        }
        unsafe {
            core::ptr::copy_nonoverlapping(pixels.as_ptr(), self.fb.add(line * WIDTH + start), len);
        }
    }
}
