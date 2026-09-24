//! Windows 窗口截图后端:`PrintWindow`(PW_RENDERFULLCONTENT)把目标窗口的
//! **客户区**渲染到离屏 DC,窗口被其他窗口遮挡也能截(个别硬件加速合成窗口
//! 可能得到黑块,属系统限制)。输出 **BGRA**,坐标为窗口相对(客户区左上为原点),
//! 配合 [`Finder`](crate::Finder) 使用时无需再手动换算屏幕偏移。

use crate::capture::Capture;
use crate::frame::Frame;
use crate::{Error, Result};

use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{BOOL, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
// windows 0.51 的元数据将 PrintWindow 归在 Win32::Storage::Xps 下(历史遗留分类)。
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetClientRect, PW_RENDERFULLCONTENT};

/// 原生窗口句柄(与 `HWND` 底层一致,避免把 `windows` crate 类型泄漏进公共 API)。
pub type WindowHandle = isize;

fn to_hwnd(hwnd: WindowHandle) -> HWND {
    HWND(hwnd)
}

/// 复用 DC/位图的 PrintWindow 抓窗器,窗口尺寸变化时自动重建位图。
struct PrintCap {
    hwnd: HWND,
    hdc_win: HDC,
    hdc_mem: HDC,
    hbmp: HBITMAP,
    old: HGDIOBJ,
    w: usize,
    h: usize,
    bmi: BITMAPINFO,
}

impl PrintCap {
    fn new(hwnd: WindowHandle) -> Self {
        unsafe {
            let hw = to_hwnd(hwnd);
            let hdc_win = GetDC(hw);
            let hdc_mem = CreateCompatibleDC(hdc_win);
            let (w, h) = client_size(hw);
            let hbmp = CreateCompatibleBitmap(hdc_win, w.max(1) as i32, h.max(1) as i32);
            let old = SelectObject(hdc_mem, hbmp);
            Self {
                hwnd: hw,
                hdc_win,
                hdc_mem,
                hbmp,
                old,
                w,
                h,
                bmi: make_bmi(w.max(1), h.max(1)),
            }
        }
    }

    /// 客户区尺寸变化时重建位图与 BITMAPINFO。
    fn resize_if_needed(&mut self) {
        let (w, h) = client_size(self.hwnd);
        if w == self.w && h == self.h {
            return;
        }
        unsafe {
            let new_hbmp = CreateCompatibleBitmap(self.hdc_win, w.max(1) as i32, h.max(1) as i32);
            SelectObject(self.hdc_mem, new_hbmp);
            let _ = DeleteObject(self.hbmp);
            self.hbmp = new_hbmp;
            self.bmi = make_bmi(w.max(1), h.max(1));
        }
        self.w = w;
        self.h = h;
    }

    /// PrintWindow 渲染客户区到内存位图,再 `GetDIBits` 直写 `dst`(BGRA)。
    fn grab_into(&mut self, dst: &mut Vec<u8>) -> Result<()> {
        self.resize_if_needed();
        if self.w == 0 || self.h == 0 {
            return Err(Error::Capture(
                "window has an empty client area (minimized?)".into(),
            ));
        }
        unsafe {
            // PW_RENDERFULLCONTENT(2):让 DirectComposition/Chromium 系窗口也走渲染路径。
            let ok: BOOL = PrintWindow(
                self.hwnd,
                self.hdc_mem,
                PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT),
            );
            if !ok.as_bool() {
                return Err(Error::Capture("PrintWindow failed".into()));
            }
            let ret = GetDIBits(
                self.hdc_mem,
                self.hbmp,
                0,
                self.h as u32,
                Some(dst.as_mut_ptr() as *mut c_void),
                &mut self.bmi,
                DIB_RGB_COLORS,
            );
            if ret == 0 {
                return Err(Error::Capture("GetDIBits failed".into()));
            }
        }
        Ok(())
    }
}

impl Drop for PrintCap {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.hdc_mem, self.old);
            let _ = DeleteObject(self.hbmp);
            let _ = DeleteDC(self.hdc_mem);
            ReleaseDC(self.hwnd, self.hdc_win);
        }
    }
}

fn client_size(hwnd: HWND) -> (usize, usize) {
    unsafe {
        let mut rc = RECT::default();
        if GetClientRect(hwnd, &mut rc).is_err() {
            return (0, 0);
        }
        (
            (rc.right - rc.left).max(0) as usize,
            (rc.bottom - rc.top).max(0) as usize,
        )
    }
}

fn make_bmi(w: usize, h: usize) -> BITMAPINFO {
    unsafe {
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w as i32;
        bmi.bmiHeader.biHeight = -(h as i32); // 负 = 自顶向下,免翻转
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = 0; // BI_RGB
        bmi
    }
}

/// [`Capture`] 的 PrintWindow 实现:截指定窗口的客户区,产出 BGRA [`Frame`]。
///
/// 与全屏后端不同,这里的 [`Match`](crate::Match) 坐标是**窗口相对**的;
/// 需要屏幕绝对坐标时叠加窗口左上角偏移即可。
pub struct WindowCapture {
    inner: PrintCap,
}

impl WindowCapture {
    /// 用原生窗口句柄创建(如从其他库拿到的 `HWND.0 as isize`)。
    pub fn new(hwnd: WindowHandle) -> Self {
        WindowCapture {
            inner: PrintCap::new(hwnd),
        }
    }

    /// 按**窗口标题**查找顶层窗口并创建(标题精确匹配,找不到返回 Err)。
    ///
    /// 便捷入口;生产环境更推荐自己枚举/持有句柄后用 [`WindowCapture::new`]。
    pub fn from_title(title: &str) -> Result<Self> {
        let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let hw = FindWindowW(PCWSTR::null(), PCWSTR(wide.as_ptr()));
            if hw.0 == 0 {
                return Err(Error::Capture(format!(
                    "no top-level window with title {title:?}"
                )));
            }
            Ok(Self::new(hw.0 as isize))
        }
    }

    /// 当前客户区尺寸(尺寸变化后需重新 `grab` 才会更新)。
    pub fn size(&self) -> (usize, usize) {
        (self.inner.w, self.inner.h)
    }
}

impl Capture for WindowCapture {
    fn grab(&mut self) -> Result<Frame> {
        self.inner.resize_if_needed();
        let (w, h) = (self.inner.w, self.inner.h);
        if w == 0 || h == 0 {
            return Err(Error::Capture(
                "window has an empty client area (minimized?)".into(),
            ));
        }
        let mut pixels = vec![0u8; w * h * 4];
        self.inner.grab_into(&mut pixels)?;
        Ok(Frame::bgra8(w, h, pixels))
    }

    fn grab_into(&mut self, dst: &mut Frame) -> Result<bool> {
        self.inner.resize_if_needed();
        let (w, h) = (self.inner.w, self.inner.h);
        if w == 0 || h == 0 {
            return Err(Error::Capture(
                "window has an empty client area (minimized?)".into(),
            ));
        }
        dst.prepare_bgra(w, h);
        self.inner.grab_into(&mut dst.pixels)?;
        Ok(true) // PrintWindow 无从判断是否变化,保守视为已变
    }

    fn backend(&self) -> &'static str {
        "print-window"
    }
}
