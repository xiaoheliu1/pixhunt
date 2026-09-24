//! 高层封装:把"截图"和"匹配"拼成一步到位的 [`Finder`]。

use crate::capture::{Capture, ScreenshotsCapture};
use crate::frame::{Frame, Rect};
use crate::matcher::{Match, Matcher, RgbMatcher};
use crate::template::Template;
use crate::Result;

use std::time::{Duration, Instant};

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
    /// PrintWindow 截指定窗口客户区(仅 Windows,遮挡也可截;坐标为窗口相对)。
    /// 需 feature `capture-window`。
    #[cfg(all(windows, feature = "capture-window"))]
    Window(crate::capture_window::WindowHandle),
    /// 自动选最快可用:DXGI → GDI → screenshots。需至少一个 Windows 后端 feature。
    #[cfg(all(windows, any(feature = "capture-gdi", feature = "capture-dxgi")))]
    Auto,
}

/// 可选的匹配算法。
pub enum MatchKind {
    /// 极速 RGB 比对。`tolerance` 为每通道最大绝对差(0=精确,~25≈容差 0.1)。
    Rgb { tolerance: i32 },
    /// 基于 corrmatch 的 ZNCC(灰度,抗光照/轻微缩放变化)。需 feature `match-corr`。
    #[cfg(feature = "match-corr")]
    Corr,
}

/// 组装好的找图器。内部**复用一个 [`Frame`]**,逐帧查找不再重复分配像素缓冲。
pub struct Finder {
    capture: Box<dyn Capture>,
    matcher: Box<dyn Matcher>,
    region: Option<Rect>,
    frame: Frame,
    cache_key: Option<u64>,
    cache_result: Option<Option<Match>>,
}

impl Finder {
    pub fn builder() -> FinderBuilder {
        FinderBuilder {
            capture: None,
            matcher: None,
            region: None,
        }
    }

    /// 用现成的后端 + 匹配器组装(region 默认整屏)。
    pub fn new(capture: Box<dyn Capture>, matcher: Box<dyn Matcher>) -> Self {
        Finder {
            capture,
            matcher,
            region: None,
            frame: Frame::bgra8(0, 0, Vec::new()),
            cache_key: None,
            cache_result: None,
        }
    }

    /// 截一屏并查找模板(若设了 `region` 则只在该区域内找)。
    ///
    /// 当后端报告画面自上次以来**未变化**、且模板与区域都和上次相同,则直接复用
    /// 上次结果、跳过搜索(静态桌面轮询的常见加速;结果与重新搜一遍完全一致)。
    pub fn find_on_screen(&mut self, tpl: &Template) -> Result<Option<Match>> {
        let changed = self.capture.grab_into(&mut self.frame)?;
        let key = self.key_of(tpl);
        if !changed && self.cache_key == Some(key) {
            if let Some(prev) = self.cache_result {
                return Ok(prev);
            }
        }
        let m = self.find_in_frame(&self.frame, tpl);
        self.cache_key = Some(key);
        self.cache_result = Some(m);
        Ok(m)
    }

    /// 轮询等待模板**出现**:每 `interval` 查一次,命中立即返回;超过 `timeout`
    /// 仍未出现返回 `Ok(None)`。自动化脚本"等按钮出现"的标准姿势,配合后端
    /// 的静态帧跳过,等待期间开销极小。
    pub fn find_until(
        &mut self,
        tpl: &Template,
        timeout: Duration,
        interval: Duration,
    ) -> Result<Option<Match>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(m) = self.find_on_screen(tpl)? {
                return Ok(Some(m));
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            std::thread::sleep(interval.min(deadline - now));
        }
    }

    /// 轮询等待模板**消失**:在 `timeout` 内找不到即返回 `Ok(true)`,超时仍能找到
    /// 返回 `Ok(false)`(如等加载遮罩消失、等按钮置灰图标下屏)。
    pub fn wait_gone(
        &mut self,
        tpl: &Template,
        timeout: Duration,
        interval: Duration,
    ) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.find_on_screen(tpl)?.is_none() {
                return Ok(true);
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(false);
            }
            std::thread::sleep(interval.min(deadline - now));
        }
    }

    /// 截一屏并找全部不重叠匹配(至多 `max` 个,`max=0` 不限)。
    pub fn find_all_on_screen(&mut self, tpl: &Template, max: usize) -> Result<Vec<Match>> {
        let changed = self.capture.grab_into(&mut self.frame)?;
        let _ = changed;
        let region = self.region.unwrap_or_else(|| self.frame.full_rect());
        Ok(self.matcher.find_all(&self.frame, tpl, region, max))
    }

    /// 只截一屏,依次匹配多个模板(省掉重复截图)。
    pub fn find_many_on_screen(&mut self, tpls: &[&Template]) -> Result<Vec<Option<Match>>> {
        self.capture.grab_into(&mut self.frame)?;
        Ok(tpls
            .iter()
            .map(|t| self.find_in_frame(&self.frame, t))
            .collect())
    }

    /// 在给定帧里查找模板(不涉及截图,便于测试/离线;遵循已设 region)。
    pub fn find_in_frame(&self, frame: &Frame, tpl: &Template) -> Option<Match> {
        match self.region {
            Some(r) => self.matcher.find_in(frame, tpl, r),
            None => self.matcher.find(frame, tpl),
        }
    }

    /// 当前限定区域(若有)。
    pub fn region(&self) -> Option<Rect> {
        self.region
    }

    /// 设置/清除限定区域(`None`=整屏)。会作废内部结果缓存。
    pub fn set_region(&mut self, region: Option<Rect>) {
        self.region = region;
        self.cache_key = None;
        self.cache_result = None;
    }

    /// 缓存键 = 模板内容指纹 ^ 区域指纹。
    fn key_of(&self, tpl: &Template) -> u64 {
        let rk = match self.region {
            None => 0u64,
            Some(r) => {
                (r.x as u64).wrapping_mul(1000003)
                    ^ (r.y as u64).wrapping_mul(7349287)
                    ^ (r.width as u64).wrapping_mul(911)
                    ^ (r.height as u64)
            }
        };
        tpl.content_key() ^ rk.rotate_left(32)
    }
}

/// [`Finder`] 的构建器。
pub struct FinderBuilder {
    capture: Option<CaptureKind>,
    matcher: Option<MatchKind>,
    region: Option<Rect>,
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
    /// 限定查找区域(绝对像素坐标),之后的 `find_*` 只在此范围内搜索。
    /// 接受 [`Rect`] 或 `(x, y, width, height)` 元组。
    pub fn region(mut self, r: impl Into<Rect>) -> Self {
        self.region = Some(r.into());
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
            #[cfg(all(windows, feature = "capture-window"))]
            CaptureKind::Window(h) => Box::new(crate::capture_window::WindowCapture::new(h)),
            #[cfg(all(windows, any(feature = "capture-gdi", feature = "capture-dxgi")))]
            CaptureKind::Auto => auto_capture()?,
        };
        let matcher: Box<dyn Matcher> =
            match self.matcher.unwrap_or(MatchKind::Rgb { tolerance: 25 }) {
                MatchKind::Rgb { tolerance } => Box::new(RgbMatcher::new(tolerance)),
                #[cfg(feature = "match-corr")]
                MatchKind::Corr => Box::new(CorrMatcher::new()),
            };
        let mut f = Finder::new(capture, matcher);
        f.region = self.region;
        Ok(f)
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

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 32;
    const H: usize = 32;

    /// 造一帧:可选在 (10,10) 贴一块 8x8 纯红(全部平台可跑,不碰真屏幕)。
    fn frame(with_target: bool) -> Frame {
        let mut px = vec![0u8; W * H * 4];
        if with_target {
            for y in 10..18 {
                for x in 10..18 {
                    let i = (y * W + x) * 4;
                    px[i] = 255;
                    px[i + 1] = 0;
                    px[i + 2] = 0;
                    px[i + 3] = 255;
                }
            }
        }
        Frame::rgba8(W, H, px)
    }

    /// 按脚本序列出帧的 mock 后端:第 i 次 grab 用 `present[i]` 决定有无目标,
    /// 序列耗尽后重复最后一项;同时统计被调用了多少次。
    struct SeqCapture {
        present: Vec<bool>,
        grabs: usize,
    }

    impl Capture for SeqCapture {
        fn grab(&mut self) -> Result<Frame> {
            let i = self.grabs.min(self.present.len() - 1);
            self.grabs += 1;
            Ok(frame(self.present[i]))
        }
    }

    fn target_tpl() -> Template {
        Template::from_rgb([255, 0, 0].repeat(8 * 8), 8, 8)
    }

    fn finder_with(present: Vec<bool>) -> Finder {
        Finder::new(
            Box::new(SeqCapture { present, grabs: 0 }),
            Box::new(RgbMatcher::new(0)),
        )
    }

    #[test]
    fn find_until_hits_after_a_few_misses() {
        let mut f = finder_with(vec![false, false, true]);
        let m = f
            .find_until(
                &target_tpl(),
                Duration::from_secs(5),
                Duration::from_millis(2),
            )
            .expect("mock 不会出错")
            .expect("第 3 帧应命中");
        assert_eq!((m.x, m.y), (10, 10));
    }

    #[test]
    fn find_until_times_out() {
        let mut f = finder_with(vec![false]);
        let m = f
            .find_until(
                &target_tpl(),
                Duration::from_millis(30),
                Duration::from_millis(5),
            )
            .expect("mock 不会出错");
        assert!(m.is_none(), "一直找不到应在超时后返回 None");
    }

    #[test]
    fn wait_gone_returns_true_and_false() {
        // 目标先出现后消失 -> 等消失成功
        let mut f = finder_with(vec![true, true, false]);
        assert!(f
            .wait_gone(
                &target_tpl(),
                Duration::from_secs(5),
                Duration::from_millis(2)
            )
            .unwrap());
        // 目标一直在 -> 超时失败
        let mut f2 = finder_with(vec![true]);
        assert!(!f2
            .wait_gone(
                &target_tpl(),
                Duration::from_millis(30),
                Duration::from_millis(5)
            )
            .unwrap());
    }
}
