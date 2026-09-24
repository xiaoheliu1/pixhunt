//! Windows GDI 截图后端:复用 DC/位图/buffer + BitBlt(1:1) + 负 biHeight 免翻转,
//! 输出 **BGRA**。相比 `screenshots` 库消除了每帧重建对象与多次全帧拷贝的开销。

use crate::capture::Capture;
use crate::frame::Frame;
use crate::Result;

use std::ffi::c_void;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, DIB_RGB_COLORS,
    GetDC, GetDIBits, GetDeviceCaps, HORZRES, ReleaseDC, SelectObject, SRCCOPY, VERTRES,
    BITMAPINFO, BITMAPINFOHEADER, HBITMAP, HDC, HGDIOBJ,
};

/// 复用型 GDI 抓屏器(内部对象只建一次)。
struct FastCap {
    hdc_screen: HDC,
    hdc_mem: HDC,
    hbmp: HBITMAP,
    old: HGDIOBJ,
    w: usize,
    h: usize,
    buf: Vec<u8>,
    bmi: BITMAPINFO,
}

impl FastCap {
    fn new_primary() -> Self {
        unsafe {
            let hdc_screen = GetDC(HWND::default());
            let w = GetDeviceCaps(hdc_screen, HORZRES) as usize;
            let h = GetDeviceCaps(hdc_screen, VERTRES) as usize;
            let hdc_mem = CreateCompatibleDC(hdc_screen);
            let hbmp = CreateCompatibleBitmap(hdc_screen, w as i32, h as i32);
            let old = SelectObject(hdc_mem, hbmp);

            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = w as i32;
            bmi.bmiHeader.biHeight = -(h as i32); // 负 = 自顶向下,免去翻转
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = 0; // BI_RGB

            Self { hdc_screen, hdc_mem, hbmp, old, w, h, buf: vec![0u8; w * h * 4], bmi }
        }
    }

    /// 抓一帧,返回 (BGRA 切片, w, h);复用内部 buffer。
    fn grab(&mut self) -> (&[u8], usize, usize) {
        unsafe {
            let _ = BitBlt(
                self.hdc_mem,
                0,
                0,
                self.w as i32,
                self.h as i32,
                self.hdc_screen,
                0,
                0,
                SRCCOPY,
            );
            let ret = GetDIBits(
                self.hdc_mem,
                self.hbmp,
                0,
                self.h as u32,
                Some(self.buf.as_mut_ptr() as *mut c_void),
                &mut self.bmi,
                DIB_RGB_COLORS,
            );
            assert!(ret != 0, "GetDIBits 失败");
            (&self.buf, self.w, self.h)
        }
    }
}

impl Drop for FastCap {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.hdc_mem, self.old);
            let _ = DeleteObject(self.hbmp);
            let _ = DeleteDC(self.hdc_mem);
            ReleaseDC(HWND::default(), self.hdc_screen);
        }
    }
}

/// [`Capture`] 的 GDI 实现,产出 BGRA [`Frame`]。
pub struct GdiCapture {
    inner: FastCap,
}

impl GdiCapture {
    /// 在主显示器上创建(仅 Windows)。
    pub fn new_primary() -> Self {
        GdiCapture { inner: FastCap::new_primary() }
    }
}

impl Capture for GdiCapture {
    fn grab(&mut self) -> Result<Frame> {
        let (bgra, w, h) = self.inner.grab();
        Ok(Frame::bgra8(w, h, bgra.to_vec()))
    }
}
