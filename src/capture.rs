//! 截图"插座":怎么拿到一帧画面。

use crate::frame::{Frame, Rect};
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

    /// 只抓取 `rect` 指定的**屏幕绝对坐标**区域,产出局部帧(区域找图时省一次全屏
    /// 拷贝)。返回 `Ok(None)` 表示该后端不支持直接区域抓取(或区域越界/跨屏),
    /// 上层应回退为"抓全屏再裁剪"。
    ///
    /// 注意:成功时产出的帧原点是 `rect` 左上角,坐标需由上层加回偏移。
    fn grab_region(&mut self, rect: Rect) -> Result<Option<Frame>> {
        let _ = rect;
        Ok(None)
    }

    /// 后端名(用于日志/诊断),默认 `"unknown"`。
    fn backend(&self) -> &'static str {
        "unknown"
    }
}

/// 把 `xcap` 的错误包成带**步骤上下文**的 [`Error::Capture`]。
///
/// `step` 写清是哪次调用失败(单看 `xcap error` 无法区分枚举显示器与抓帧),
/// 原始错误保留在 `source`:xcap 自己还会再包一层 io / D-Bus / Win32 错误,
/// 断链后这些才是真正可诊断的信息就取不出来了。
fn xcap_err(step: impl Into<String>, e: xcap::XCapError) -> Error {
    Error::capture_from(format!("xcap {}", step.into()), e)
}

/// 基于 [`xcap`](https://crates.io/crates/xcap) 的跨平台保底后端(输出 RGBA)。
///
/// 一个实例绑定一个显示器,`Monitor` 对象**可长期复用**(内部只存显示器句柄与几何
/// 信息),每帧 `grab` 不会重新枚举显示器。
pub struct XCapCapture {
    monitor: xcap::Monitor,
}

impl XCapCapture {
    /// 主显示器(拿不到主显示器标记时退化为枚举到的第一个)。
    pub fn primary() -> Result<Self> {
        let monitors = xcap::Monitor::all().map_err(|e| xcap_err("Monitor::all()", e))?;
        let monitor = monitors
            .iter()
            .find(|m| m.is_primary().unwrap_or(false))
            .or_else(|| monitors.first())
            .cloned()
            .ok_or_else(|| Error::capture("no display found"))?;
        Ok(XCapCapture { monitor })
    }

    /// 包含给定点(屏幕绝对坐标)的显示器——多屏时用。
    pub fn from_point(x: i32, y: i32) -> Result<Self> {
        Ok(XCapCapture {
            monitor: xcap::Monitor::from_point(x, y)
                .map_err(|e| xcap_err(format!("Monitor::from_point({x}, {y})"), e))?,
        })
    }

    /// 当前在线显示器个数。
    pub fn monitor_count() -> Result<usize> {
        Ok(xcap::Monitor::all()
            .map_err(|e| xcap_err("Monitor::all()", e))?
            .len())
    }
}

impl Capture for XCapCapture {
    fn grab(&mut self) -> Result<Frame> {
        let img = self
            .monitor
            .capture_image()
            .map_err(|e| xcap_err("Monitor::capture_image()", e))?;
        Ok(Frame::rgba8(
            img.width() as usize,
            img.height() as usize,
            img.into_raw(),
        ))
    }

    fn grab_region(&mut self, rect: Rect) -> Result<Option<Frame>> {
        // xcap 的区域接口以**显示器左上角**为原点且不接受负值,而 [`Rect`] 用的是屏幕
        // 绝对坐标(usize)。两者只在"显示器原点恰为 (0,0)"时一致(即主屏),其余情况
        // (副屏带偏移、跨屏区域)直接回退全屏路径。
        if self.monitor.x().map_err(|e| xcap_err("Monitor::x()", e))? != 0
            || self.monitor.y().map_err(|e| xcap_err("Monitor::y()", e))? != 0
        {
            return Ok(None);
        }
        let (mw, mh) = (
            self.monitor
                .width()
                .map_err(|e| xcap_err("Monitor::width()", e))? as u64,
            self.monitor
                .height()
                .map_err(|e| xcap_err("Monitor::height()", e))? as u64,
        );
        let (x, y, w, h) = (
            rect.x as u64,
            rect.y as u64,
            rect.width as u64,
            rect.height as u64,
        );
        if w == 0 || h == 0 || x + w > mw || y + h > mh {
            return Ok(None);
        }
        let img = self
            .monitor
            .capture_region(x as u32, y as u32, w as u32, h as u32)
            .map_err(|e| xcap_err(format!("Monitor::capture_region({x}, {y}, {w}, {h})"), e))?;
        Ok(Some(Frame::rgba8(
            img.width() as usize,
            img.height() as usize,
            img.into_raw(),
        )))
    }

    fn backend(&self) -> &'static str {
        "xcap"
    }
}
