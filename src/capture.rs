//! 截图"插座":怎么拿到一帧画面。

use crate::frame::Frame;
use crate::{Error, Result};

/// 可插拔的截图后端。实现它即可接入新的抓屏方式(如 GDI / DXGI)。
pub trait Capture {
    /// 抓取一帧,产出 [`Frame`]。
    fn grab(&mut self) -> Result<Frame>;
}

/// 基于 `screenshots` 库的跨平台保底后端(输出 RGBA)。
pub struct ScreenshotsCapture {
    screen: screenshots::Screen,
}

impl ScreenshotsCapture {
    /// 使用主显示器。
    pub fn primary() -> Result<Self> {
        let screens =
            screenshots::Screen::all().map_err(|e| Error::Capture(e.to_string()))?;
        let screen = screens
            .into_iter()
            .next()
            .ok_or_else(|| Error::Capture("no display found".into()))?;
        Ok(ScreenshotsCapture { screen })
    }
}

impl Capture for ScreenshotsCapture {
    fn grab(&mut self) -> Result<Frame> {
        let img = self.screen.capture().map_err(|e| Error::Capture(e.to_string()))?;
        let (width, height) = (img.width() as usize, img.height() as usize);
        Ok(Frame::rgba8(width, height, img.into_raw()))
    }
}

