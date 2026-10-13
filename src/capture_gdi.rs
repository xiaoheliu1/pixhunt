//! Windows GDI 截图后端:复用 DC/位图 + BitBlt(1:1) + 负 biHeight 免翻转,
//! 输出 **BGRA**。相比跨平台保底后端(`XCapCapture`)消除了每帧重建对象的开销;`grab_into`
//! 更把像素直接 `GetDIBits` 写进调用方缓冲,连内部中转 buffer 都省了。

use crate::capture::Capture;
use crate::frame::Frame;
use crate::{Error, Result};

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

    /// 桌面尺寸变化(改分辨率、改缩放比例、切换主显示器、RDP 重连)后重建位图与 bmi。
    ///
    /// 不处理的话后果很隐蔽:位图还是旧尺寸,`BitBlt` 只填旧区域、`GetDIBits` 又按旧
    /// `bmi` 读,画面从此**错位且不会自行恢复**,除非重建整个 `GdiCapture`。
    /// 代价是每次抓取多两次 `GetDeviceCaps` 调用,实测可忽略。
    fn resize_if_needed(&mut self) {
        unsafe {
            let w = GetDeviceCaps(self.hdc_screen, HORZRES) as usize;
            let h = GetDeviceCaps(self.hdc_screen, VERTRES) as usize;
            if w == 0 || h == 0 || (w == self.w && h == self.h) {
                return;
            }
            let new_hbmp = CreateCompatibleBitmap(self.hdc_screen, w as i32, h as i32);
            if new_hbmp.is_invalid() {
                // 建不出来就沿用旧对象,让后面的 BitBlt / GetDIBits 走正常报错路径。
                return;
            }
            // 换入新位图;被换出的那张此刻不属于任何 DC,可以安全删除。
            SelectObject(self.hdc_mem, new_hbmp);
            let _ = DeleteObject(self.hbmp);
            self.hbmp = new_hbmp;
            self.bmi.bmiHeader.biWidth = w as i32;
            self.bmi.bmiHeader.biHeight = -(h as i32); // 负 = 自顶向下
            self.w = w;
            self.h = h;
        }
    }

    /// 抓一帧,BGRA 直接写入 `dst`。`dst` 会被就地扩到 `w*h*4`(长度已够时
    /// `resize` 是 no-op,复用原容量、不重新分配)。
    ///
    /// GDI 失败返回 `Err` 而**不是 panic**:显示器休眠、分辨率切换、无人登录的
    /// 活动会话等瞬态场景都是可恢复错误,不该让调用方进程跟着崩。
    fn grab_into(&mut self, dst: &mut Vec<u8>) -> Result<()> {
        self.resize_if_needed();
        // 先保证长度:GetDIBits 按 bmi 从指针起直写 w*h*4 字节,缓冲短了就是堆溢出写。
        dst.resize(self.w * self.h * 4, 0);
        unsafe {
            if BitBlt(
                self.hdc_mem,
                0,
                0,
                self.w as i32,
                self.h as i32,
                self.hdc_screen,
                0,
                0,
                SRCCOPY,
            )
            .is_err()
            {
                return Err(Error::capture("BitBlt failed"));
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
                return Err(Error::capture("GetDIBits failed"));
            }
        }
        Ok(())
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
    ///
    /// 尺寸取的是 `GetDC(NULL)` + `HORZRES/VERTRES`,即**主显示器那一块**,不含副屏,
    /// 也不含负坐标区域。多屏时要按坐标选屏请用 `XCapCapture::from_point`,要盯
    /// 某个窗口请用 `WindowCapture`。
    pub fn new_primary() -> Self {
        GdiCapture {
            inner: FastCap::new_primary(),
        }
    }
}

impl Capture for GdiCapture {
    fn grab(&mut self) -> Result<Frame> {
        // 先跟到当前桌面尺寸再取尺寸:`FastCap::grab_into` 自己也会 resize,它跑在
        // 前面时才能交出与缓冲一致的尺寸(见 `grab_into` 的说明)。
        self.inner.resize_if_needed();
        let (w, h) = self.inner.size();
        // 缓冲由 FastCap::grab_into 负责扩到 w*h*4,这里不必先填一遍零。
        let mut pixels = Vec::new();
        self.inner.grab_into(&mut pixels)?;
        debug_assert_eq!(pixels.len(), w * h * 4, "抓屏写出的帧尺寸与缓冲不一致");
        Ok(Frame::bgra8(w, h, pixels))
    }

    fn grab_into(&mut self, dst: &mut Frame) -> Result<bool> {
        // 顺序不能反!`FastCap::grab_into` 开头会 `resize_if_needed()`,并把
        // `dst.pixels` 按**它自己的**新尺寸 `resize`。如果像旧代码那样先读旧尺寸,
        // 分辨率一变(改分辨率 / 改缩放 / 切主屏 / RDP 重连)就会出现
        // `dst.width*height` 是新尺寸而像素是旧尺寸(或反过来)的帧:
        // 前者是静默的错位画面(行距对不上,匹配结果乱跳),后者会在
        // 匹配路径的切片下标上直接 panic —— 与本文件"GDI 失败返 Err 而不是 panic"
        // 的约定相违。先 resize 再取尺寸,两边就总是同一个值。
        self.inner.resize_if_needed();
        let (w, h) = self.inner.size();
        dst.prepare_bgra(w, h);
        self.inner.grab_into(&mut dst.pixels)?;
        debug_assert_eq!(dst.pixels.len(), w * h * 4, "抓屏写出的帧尺寸与缓冲不一致");
        Ok(true) // GDI 无从判断画面是否变化,保守视为已变
    }

    fn backend(&self) -> &'static str {
        "gdi"
    }
}
