//! Windows GDI 截图后端:复用 DC/位图 + BitBlt(1:1) + 负 biHeight 免翻转,
//! 输出 **BGRA**。相比 `screenshots` 库消除了每帧重建对象的开销;`grab_into`
//! 更把像素直接 `GetDIBits` 写进调用方缓冲,连内部中转 buffer 都省了。

use crate::capture::Capture;
use crate::frame::Frame;
use crate::Result;

use std::ffi::c_void;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    GetDeviceCaps, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HBITMAP,
    HDC, HGDIOBJ, HORZRES, SRCCOPY, VERTRES,
};

/// 复用型 GDI 抓屏器(内部对象只建一次)。
struct FastCap {
    hdc_screen: HDC,
    hdc_mem: HDC,
    hbmp: HBITMAP,
    old: HGDIOBJ,
    w: usize,
    h: usize,
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

            Self {
                hdc_screen,
                hdc_mem,
                hbmp,
                old,
                w,
                h,
                bmi,
            }
        }
    }

    fn size(&self) -> (usize, usize) {
        (self.w, self.h)
    }

    /// 抓一帧,BGRA 直接写入 `dst`(`dst` 长度会被设为 `w*h*4`)。
    fn grab_into(&mut self, dst: &mut Vec<u8>) {
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
                Some(dst.as_mut_ptr() as *mut c_void),
                &mut self.bmi,
                DIB_RGB_COLORS,
            );
            assert!(ret != 0, "GetDIBits 失败");
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
        GdiCapture {
            inner: FastCap::new_primary(),
        }
    }
}

impl Capture for GdiCapture {
    fn grab(&mut self) -> Result<Frame> {
        let (w, h) = self.inner.size();
        let mut pixels = vec![0u8; w * h * 4];
        self.inner.grab_into(&mut pixels);
        Ok(Frame::bgra8(w, h, pixels))
    }

    fn grab_into(&mut self, dst: &mut Frame) -> Result<bool> {
        let (w, h) = self.inner.size();
        dst.prepare_bgra(w, h);
        self.inner.grab_into(&mut dst.pixels);
        Ok(true) // GDI 无从判断画面是否变化,保守视为已变
    }
}
