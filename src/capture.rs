//! 截图"插座":怎么拿到一帧画面。

use crate::frame::Frame;
use crate::{Error, Result};

/// 可插拔的截图后端。实现 [`Capture::grab`] 即可接入新的抓屏方式(如 GDI / DXGI)。
pub trait Capture {
    /// 抓取一帧,产出 [`Frame`]。
    fn grab(&mut self) -> Result<Frame>;

    /// 复用 `dst` 的像素缓冲抓一帧,返回**画面相对上次是否发生变化**。
    ///
    /// 默认实现直接 [`grab`](Capture::grab) 并覆盖 `dst`(视为已变化)。快后端
    /// (GDI / DXGI)可覆写:把像素直接写进 `dst.pixels` 以免每帧重新分配,并在
    /// 确实没有新帧(如 DXGI `WAIT_TIMEOUT`)时返回 `false` 以便上层跳过搜索。
    fn grab_into(&mut self, dst: &mut Frame) -> Result<bool> {
        *dst = self.grab()?;
        Ok(true)
    }

    /// 后端名(用于日志/诊断),默认 `"unknown"`。
    fn backend(&self) -> &'static str {
        "unknown"
    }
}

/// 基于 `screenshots` 库的跨平台保底后端(输出 RGBA)。
pub struct ScreenshotsCapture {
    screen: screenshots::Screen,
}

impl ScreenshotsCapture {
    /// 使用主显示器。
    pub fn primary() -> Result<Self> {
        let screens = screenshots::Screen::all().map_err(|e| Error::Capture(e.to_string()))?;
        let screen = screens
            .into_iter()
            .next()
            .ok_or_else(|| Error::Capture("no display found".into()))?;
        Ok(ScreenshotsCapture { screen })
    }
}

impl Capture for ScreenshotsCapture {
    fn grab(&mut self) -> Result<Frame> {
        let img = self
            .screen
            .capture()
            .map_err(|e| Error::Capture(e.to_string()))?;
        let (width, height) = (img.width() as usize, img.height() as usize);
        Ok(Frame::rgba8(width, height, img.into_raw()))
    }

    fn backend(&self) -> &'static str {
        "screenshots"
    }
}
