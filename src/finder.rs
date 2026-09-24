//! 高层封装:把"截图"和"匹配"拼成一步到位的 [`Finder`]。

use crate::capture::{Capture, ScreenshotsCapture};
use crate::frame::Frame;
use crate::matcher::{Match, Matcher, RgbMatcher};
use crate::template::Template;
use crate::Result;

#[cfg(all(windows, feature = "capture-dxgi"))]
use crate::Error;

#[cfg(feature = "match-corr")]
use crate::matcher_corr::CorrMatcher;

/// 可选的截图后端。
pub enum CaptureKind {
    /// 跨平台保底(基于 `screenshots` 库,输出 RGBA)。
    Screenshots,
    /// 复用型 GDI BitBlt(仅 Windows,输出 BGRA)。需 feature `capture-gdi`。
    #[cfg(all(windows, feature = "capture-gdi"))]
    Gdi,
    /// DXGI 桌面复制(仅 Windows,最快,输出 BGRA)。需 feature `capture-dxgi`。
    #[cfg(all(windows, feature = "capture-dxgi"))]
    Dxgi,
    /// 自动选最快可用:DXGI → GDI → screenshots。需至少一个 Windows 后端 feature。
    #[cfg(all(windows, any(feature = "capture-gdi", feature = "capture-dxgi")))]
    Auto,
}

/// 可选的匹配算法。
pub enum MatchKind {
    /// 极速 RGB 比对。`tolerance` 为每通道最大绝对差。
    Rgb { tolerance: i32 },
    /// 基于 corrmatch 的 ZNCC(灰度,抗光照/轻微缩放)。需 feature `match-corr`。
    #[cfg(feature = "match-corr")]
    Corr,
}

/// 组装好的找图器。
pub struct Finder {
    capture: Box<dyn Capture>,
    matcher: Box<dyn Matcher>,
}

impl Finder {
    pub fn builder() -> FinderBuilder {
        FinderBuilder { capture: None, matcher: None }
    }

    /// 截一屏并查找模板。
    pub fn find_on_screen(&mut self, tpl: &Template) -> Result<Option<Match>> {
        let frame = self.capture.grab()?;
        Ok(self.find_in_frame(&frame, tpl))
    }

    /// 在给定帧里查找模板(不涉及截图,便于测试/离线)。
    pub fn find_in_frame(&self, frame: &Frame, tpl: &Template) -> Option<Match> {
        self.matcher.find(frame, tpl)
    }
}

/// [`Finder`] 的构建器。
pub struct FinderBuilder {
    capture: Option<CaptureKind>,
    matcher: Option<MatchKind>,
}

impl FinderBuilder {
    pub fn capture(mut self, k: CaptureKind) -> Self {
        self.capture = Some(k);
        self
    }
    pub fn matcher(mut self, k: MatchKind) -> Self {
        self.matcher = Some(k);
        self
    }
    pub fn build(self) -> Result<Finder> {
        let capture: Box<dyn Capture> = match self.capture.unwrap_or(CaptureKind::Screenshots) {
            CaptureKind::Screenshots => Box::new(ScreenshotsCapture::primary()?),
            #[cfg(all(windows, feature = "capture-gdi"))]
            CaptureKind::Gdi => Box::new(crate::capture_gdi::GdiCapture::new_primary()),
            #[cfg(all(windows, feature = "capture-dxgi"))]
            CaptureKind::Dxgi => Box::new(
                crate::capture_dxgi::DxgiCapture::new_primary()
                    .ok_or_else(|| Error::Capture("DXGI desktop duplication unavailable".into()))?,
            ),
            #[cfg(all(windows, any(feature = "capture-gdi", feature = "capture-dxgi")))]
            CaptureKind::Auto => auto_capture()?,
        };
        let matcher: Box<dyn Matcher> =
            match self.matcher.unwrap_or(MatchKind::Rgb { tolerance: 25 }) {
                MatchKind::Rgb { tolerance } => Box::new(RgbMatcher::new(tolerance)),
                #[cfg(feature = "match-corr")]
                MatchKind::Corr => Box::new(CorrMatcher::new()),
            };
        Ok(Finder { capture, matcher })
    }
}

/// 依次尝试 DXGI → GDI → screenshots,返回第一个可用的。
#[cfg(all(windows, any(feature = "capture-gdi", feature = "capture-dxgi")))]
fn auto_capture() -> Result<Box<dyn Capture>> {
    #[cfg(feature = "capture-dxgi")]
    if let Some(c) = crate::capture_dxgi::DxgiCapture::new_primary() {
        return Ok(Box::new(c));
    }
    #[cfg(feature = "capture-gdi")]
    {
        return Ok(Box::new(crate::capture_gdi::GdiCapture::new_primary()));
    }
    #[allow(unreachable_code)]
    Ok(Box::new(ScreenshotsCapture::primary()?))
}
