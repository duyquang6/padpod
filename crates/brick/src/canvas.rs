//! The panel, as a double-buffered BGRA framebuffer. Shared by every app here.
//!
//! Adapted from Truepod's canvas, which drives this same 1024x768 panel: the
//! stride comes from the driver, drawing goes to the off-screen buffer, and a
//! finished frame is made visible with one FBIOPAN_DISPLAY.
//!
//! The alpha byte matters here. The firmware's player puts video on a hardware
//! layer *underneath* this one, and this layer is blended per pixel: video
//! shows only where a pixel's alpha is 0 (measured - with the launcher's
//! opaque frame on top there was sound and no picture). So the player screen
//! clears to transparent and draws its controls opaque over that.

use std::fs::OpenOptions;
use std::io;
use std::os::unix::io::AsRawFd;

const FBIOGET_VSCREENINFO: libc::c_ulong = 0x4600;
const FBIOGET_FSCREENINFO: libc::c_ulong = 0x4602;
const FBIOPAN_DISPLAY: libc::c_ulong = 0x4606;
/// Not FB_ACTIVATE_VBL: waiting for a vertical blank can block for a very long
/// time on this display controller after a resume (Truepod's device notes).
const FB_ACTIVATE_NOW: u32 = 0;
const BPP: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// A decoded picture in the panel's own pixel order (BGRA), so drawing it is
/// a copy. Each app decodes into it in its own way.
pub struct Bitmap {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

impl Bitmap {
    pub fn new(w: usize, h: usize) -> Self {
        Self { w, h, data: vec![0; w * h * 4] }
    }
}

/// Subset of fb_var_screeninfo. Only the leading fields are read, but the
/// struct must be laid out in full so the kernel writes within bounds.
#[repr(C)]
#[derive(Default)]
struct FbVarScreeninfo {
    xres: u32,
    yres: u32,
    xres_virtual: u32,
    yres_virtual: u32,
    xoffset: u32,
    yoffset: u32,
    bits_per_pixel: u32,
    grayscale: u32,
    red: [u32; 3],
    green: [u32; 3],
    blue: [u32; 3],
    transp: [u32; 3],
    nonstd: u32,
    activate: u32,
    height: u32,
    width: u32,
    accel_flags: u32,
    pixclock: u32,
    left_margin: u32,
    right_margin: u32,
    upper_margin: u32,
    lower_margin: u32,
    hsync_len: u32,
    vsync_len: u32,
    sync: u32,
    vmode: u32,
    rotate: u32,
    colorspace: u32,
    reserved: [u32; 4],
}

#[repr(C)]
#[derive(Default)]
struct FbFixScreeninfo {
    id: [u8; 16],
    smem_start: u64,
    smem_len: u32,
    kind: u32,
    type_aux: u32,
    visual: u32,
    xpanstep: u16,
    ypanstep: u16,
    ywrapstep: u16,
    line_length: u32,
    mmio_start: u64,
    mmio_len: u32,
    accel: u32,
    capabilities: u16,
    reserved: [u16; 2],
}

enum Target {
    Memory(Vec<u8>),
    Mapped { ptr: *mut u8, len: usize, file: std::fs::File },
}

pub struct Canvas {
    width: usize,
    height: usize,
    /// Bytes per row as the driver reports it, not necessarily width * 4.
    stride: usize,
    /// 2 enables page flipping; 1 means drawing goes straight to the screen.
    buffers: usize,
    /// Which buffer drawing currently targets.
    back: usize,
    target: Target,
}

// SAFETY: the mapping is owned by the Canvas alone and never aliased, so
// handing the whole Canvas to another thread (Padpod's renderer) is sound.
unsafe impl Send for Canvas {}

impl Canvas {
    pub fn open(path: &str) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let fd = file.as_raw_fd();

        let mut var = FbVarScreeninfo::default();
        if unsafe { libc::ioctl(fd, FBIOGET_VSCREENINFO as _, &mut var) } < 0 {
            return Err(io::Error::last_os_error());
        }
        if var.bits_per_pixel != 32 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("framebuffer is {}bpp, expected 32", var.bits_per_pixel),
            ));
        }
        let mut fix = FbFixScreeninfo::default();
        if unsafe { libc::ioctl(fd, FBIOGET_FSCREENINFO as _, &mut fix) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let stride = if fix.line_length > 0 {
            fix.line_length as usize
        } else {
            var.xres as usize * BPP
        };

        let buffers = if var.yres_virtual >= var.yres * 2 { 2 } else { 1 };
        let len = stride * var.yres as usize * buffers;
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        Ok(Self {
            width: var.xres as usize,
            height: var.yres as usize,
            stride,
            buffers,
            // Start in the second screen so the first flip reveals a whole frame.
            back: buffers - 1,
            target: Target::Mapped { ptr: ptr as *mut u8, len, file },
        })
    }

    /// For running without a panel: the host-side headless mode and the tests.
    pub fn in_memory(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            stride: width * BPP,
            buffers: 1,
            back: 0,
            target: Target::Memory(vec![0; width * BPP * height]),
        }
    }

    pub fn width(&self) -> i32 {
        self.width as i32
    }

    pub fn height(&self) -> i32 {
        self.height as i32
    }

    fn back_offset(&self) -> usize {
        self.back * self.stride * self.height
    }

    fn buffer_mut(&mut self) -> &mut [u8] {
        match &mut self.target {
            Target::Memory(v) => v.as_mut_slice(),
            Target::Mapped { ptr, len, .. } => unsafe { std::slice::from_raw_parts_mut(*ptr, *len) },
        }
    }

    fn buffer(&self) -> &[u8] {
        match &self.target {
            Target::Memory(v) => v.as_slice(),
            Target::Mapped { ptr, len, .. } => unsafe { std::slice::from_raw_parts(*ptr, *len) },
        }
    }

    /// A rectangle clipped to the panel as `(x, y, w, h)`, or None if nothing
    /// of it is visible.
    fn clip(&self, x: i32, y: i32, w: i32, h: i32) -> Option<(usize, usize, usize, usize)> {
        let (left, top) = (x.max(0), y.max(0));
        let right = (x + w).min(self.width as i32);
        let bottom = (y + h).min(self.height as i32);
        if right <= left || bottom <= top {
            return None;
        }
        Some((left as usize, top as usize, (right - left) as usize, (bottom - top) as usize))
    }

    pub fn clear(&mut self, colour: Rgb) {
        self.fill_rect(0, 0, self.width as i32, self.height as i32, colour);
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, colour: Rgb) {
        let Some((x, y, w, h)) = self.clip(x, y, w, h) else { return };
        let (stride, base) = (self.stride, self.back_offset());
        let row: Vec<u8> = [colour.2, colour.1, colour.0, 0xff].iter().copied().cycle().take(w * BPP).collect();
        let buf = self.buffer_mut();
        for line in y..y + h {
            let at = base + line * stride + x * BPP;
            buf[at..at + row.len()].copy_from_slice(&row);
        }
    }

    /// Make the whole frame transparent, so the video layer underneath shows.
    pub fn clear_transparent(&mut self) {
        self.fill_transparent(0, 0, self.width as i32, self.height as i32);
    }

    /// Make a rectangle transparent.
    pub fn fill_transparent(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.fill_rect_alpha(x, y, w, h, Rgb(0, 0, 0), 0);
    }

    /// Fill a rectangle with a colour at a given layer alpha: 255 hides the
    /// video underneath, 0 shows it untouched, anything between tints it.
    pub fn fill_rect_alpha(&mut self, x: i32, y: i32, w: i32, h: i32, colour: Rgb, alpha: u8) {
        let Some((x, y, w, h)) = self.clip(x, y, w, h) else { return };
        let (stride, base) = (self.stride, self.back_offset());
        let row: Vec<u8> = [colour.2, colour.1, colour.0, alpha].iter().copied().cycle().take(w * BPP).collect();
        let buf = self.buffer_mut();
        for line in y..y + h {
            let at = base + line * stride + x * BPP;
            buf[at..at + row.len()].copy_from_slice(&row);
        }
    }

    /// Darken or tint a rectangle: `alpha` 0 leaves it alone, 255 is opaque.
    pub fn blend_rect(&mut self, x: i32, y: i32, w: i32, h: i32, colour: Rgb, alpha: u8) {
        let Some((x, y, w, h)) = self.clip(x, y, w, h) else { return };
        let (stride, base) = (self.stride, self.back_offset());
        let src = [colour.2 as u32, colour.1 as u32, colour.0 as u32];
        let a = alpha as u32;
        let buf = self.buffer_mut();
        for line in y..y + h {
            let at = base + line * stride + x * BPP;
            for px in buf[at..at + w * BPP].chunks_exact_mut(BPP) {
                for c in 0..3 {
                    px[c] = ((src[c] * a + px[c] as u32 * (255 - a)) / 255) as u8;
                }
            }
        }
    }
    /// A one-pixel-thick outline just inside the rectangle, `t` pixels wide.
    pub fn frame_rect(&mut self, x: i32, y: i32, w: i32, h: i32, t: i32, colour: Rgb) {
        self.fill_rect(x, y, w, t, colour);
        self.fill_rect(x, y + h - t, w, t, colour);
        self.fill_rect(x, y, t, h, colour);
        self.fill_rect(x + w - t, y, t, h, colour);
    }

    /// The layer alpha of one pixel in the frame being drawn.
    #[cfg(test)]
    pub fn alpha_at(&self, x: usize, y: usize) -> u8 {
        self.buffer()[self.back_offset() + y * self.stride + x * BPP + 3]
    }

    /// Blend a glyph's coverage bitmap in one call (Truepod's fast path).
    pub fn blend_mask(&mut self, x: i32, y: i32, w: usize, coverage: &[u8], colour: Rgb) {
        if w == 0 || coverage.is_empty() {
            return;
        }
        let h = coverage.len() / w;
        let (cw, ch) = (self.width as i32, self.height as i32);
        let (stride, base) = (self.stride, self.back_offset());
        let src = [colour.2, colour.1, colour.0];
        let buf = self.buffer_mut();
        for row in 0..h {
            let py = y + row as i32;
            if py < 0 || py >= ch {
                continue;
            }
            let line = base + py as usize * stride;
            for col in 0..w {
                let a = coverage[row * w + col];
                if a == 0 {
                    continue;
                }
                let px = x + col as i32;
                if px < 0 || px >= cw {
                    continue;
                }
                let i = line + px as usize * BPP;
                if a == 255 {
                    buf[i..i + 3].copy_from_slice(&src);
                } else {
                    let a = a as u32;
                    for (off, &s) in src.iter().enumerate() {
                        let dst = buf[i + off] as u32;
                        buf[i + off] = ((s as u32 * a + dst * (255 - a)) / 255) as u8;
                    }
                }
                buf[i + 3] = 0xff;
            }
        }
    }

    /// Copy a bitmap with its top-left corner at `(x, y)`, clipped to the panel.
    pub fn blit(&mut self, bmp: &Bitmap, x: i32, y: i32) {
        let Some((cx, cy, cw, ch)) = self.clip(x, y, bmp.w as i32, bmp.h as i32) else { return };
        let (sx, sy) = ((cx as i32 - x) as usize, (cy as i32 - y) as usize);
        let (stride, base) = (self.stride, self.back_offset());
        let buf = self.buffer_mut();
        for row in 0..ch {
            let src = ((sy + row) * bmp.w + sx) * BPP;
            let dst = base + (cy + row) * stride + cx * BPP;
            buf[dst..dst + cw * BPP].copy_from_slice(&bmp.data[src..src + cw * BPP]);
        }
    }

    /// Copy a bitmap turned a quarter turn clockwise, its rotated top-left
    /// corner at `(x, y)`. The rotated image is `bmp.h` wide and `bmp.w` tall.
    pub fn blit_rotated(&mut self, bmp: &Bitmap, x: i32, y: i32) {
        let Some((cx, cy, cw, ch)) = self.clip(x, y, bmp.h as i32, bmp.w as i32) else { return };
        let (stride, base) = (self.stride, self.back_offset());
        let buf = self.buffer_mut();
        for row in 0..ch {
            // Destination (X, Y) within the rotated image reads source
            // column Y, row (h - 1 - X).
            let src_col = cy + row - y as usize;
            let dst_line = base + (cy + row) * stride;
            for col in 0..cw {
                let src_row = bmp.h - 1 - (cx + col - x as usize);
                let src = (src_row * bmp.w + src_col) * BPP;
                let dst = dst_line + (cx + col) * BPP;
                buf[dst..dst + BPP].copy_from_slice(&bmp.data[src..src + BPP]);
            }
        }
    }

    /// The frame being drawn, as tightly packed RGB - for screenshots.
    pub fn snapshot_rgb(&self) -> Vec<u8> {
        let (stride, base) = (self.stride, self.back_offset());
        let buf = self.buffer();
        let mut out = Vec::with_capacity(self.width * self.height * 3);
        for y in 0..self.height {
            let row = base + y * stride;
            for px in buf[row..row + self.width * BPP].chunks_exact(BPP) {
                out.extend_from_slice(&[px[2], px[1], px[0]]);
            }
        }
        out
    }

    /// Make the drawn buffer visible, then switch drawing to the other one.
    pub fn flush(&mut self) -> io::Result<()> {
        if self.buffers < 2 {
            return Ok(());
        }
        let yoffset = (self.back * self.height) as u32;
        if let Target::Mapped { file, .. } = &mut self.target {
            let fd = file.as_raw_fd();
            // Re-read rather than reuse: the controller is reinitialised across
            // a resume, so a cached copy may no longer describe the device.
            let mut var = FbVarScreeninfo::default();
            if unsafe { libc::ioctl(fd, FBIOGET_VSCREENINFO as _, &mut var) } < 0 {
                return Err(io::Error::last_os_error());
            }
            var.yoffset = yoffset;
            var.activate = FB_ACTIVATE_NOW;
            if unsafe { libc::ioctl(fd, FBIOPAN_DISPLAY as _, &mut var as *mut FbVarScreeninfo) } < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        self.back = (self.back + 1) % self.buffers;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(c: &Canvas, x: usize, y: usize) -> (u8, u8, u8) {
        let rgb = c.snapshot_rgb();
        let i = (y * c.width + x) * 3;
        (rgb[i], rgb[i + 1], rgb[i + 2])
    }

    #[test]
    fn a_filled_rectangle_is_clipped_to_the_canvas() {
        let mut c = Canvas::in_memory(8, 8);
        c.fill_rect(-4, -4, 6, 6, Rgb(1, 2, 3));
        assert_eq!(pixel(&c, 1, 1), (1, 2, 3));
        assert_eq!(pixel(&c, 2, 2), (0, 0, 0));
        c.fill_rect(20, 20, 4, 4, Rgb(9, 9, 9));
        c.fill_rect(2, 2, 0, 0, Rgb(9, 9, 9));
        assert_eq!(pixel(&c, 3, 3), (0, 0, 0));
    }

    #[test]
    fn transparent_pixels_have_zero_alpha_and_opaque_ones_full() {
        let mut c = Canvas::in_memory(4, 4);
        c.clear(Rgb(10, 20, 30));
        assert_eq!(c.alpha_at(1, 1), 0xff);
        c.fill_transparent(0, 0, 2, 2);
        assert_eq!(c.alpha_at(1, 1), 0, "the video layer shows through here");
        assert_eq!(c.alpha_at(2, 2), 0xff, "and not here");
        c.clear_transparent();
        assert_eq!(c.alpha_at(3, 3), 0);
        // A translucent bar keeps its own alpha rather than forcing opaque.
        c.fill_rect_alpha(0, 0, 4, 1, Rgb(0, 0, 0), 160);
        assert_eq!(c.alpha_at(0, 0), 160);
        // Text over a transparent area must become visible.
        c.blend_mask(0, 3, 1, &[128], Rgb(255, 255, 255));
        assert_eq!(c.alpha_at(0, 3), 0xff);
    }

    #[test]
    fn a_rotated_blit_turns_the_image_clockwise() {
        // 2 wide, 1 tall: red then green. Clockwise, red ends up on top.
        let mut bmp = Bitmap::new(2, 1);
        bmp.data.copy_from_slice(&[0, 0, 255, 255, 0, 255, 0, 255]);
        let mut c = Canvas::in_memory(3, 3);
        c.blit_rotated(&bmp, 1, 0);
        assert_eq!(pixel(&c, 1, 0), (255, 0, 0));
        assert_eq!(pixel(&c, 1, 1), (0, 255, 0));
        assert_eq!(pixel(&c, 0, 0), (0, 0, 0));
        // Hanging off the bottom-right must not panic.
        c.blit_rotated(&bmp, 2, 2);
        assert_eq!(pixel(&c, 2, 2), (255, 0, 0));
    }
}
