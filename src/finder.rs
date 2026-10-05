//! 高层封装:把"截图"和"匹配"拼成一步到位的 [`Finder`]。

use crate::capture::{Capture, XCapCapture};
use crate::color::{self, ColorBlob, ColorSpec};
use crate::frame::{Frame, Rect};
use crate::matcher::{Match, Matcher, RgbMatcher};
use crate::template::Template;
use crate::Result;

use std::time::{Duration, Instant};

#[cfg(all(windows, feature = "capture-dxgi"))]
use crate::Error;

#[cfg(feature = "match-corr")]
use crate::matcher_corr::{CorrConfig, CorrMatcher};

/// 可选的截图后端。
pub enum CaptureKind {
    /// 跨平台保底(基于 `xcap` 库,输出 RGBA;只绑主显示器)。
    Monitor,
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
    /// 按窗口标题(**精确匹配**)截单个窗口客户区(仅 Windows,坐标为窗口相对)。
    /// 需 feature `capture-window`。
    #[cfg(all(windows, feature = "capture-window"))]
    WindowByTitle(String),
    /// 自动选最快可用:DXGI → GDI → xcap。需至少一个 Windows 后端 feature。
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
    /// 同上,但自定义搜索参数(金字塔层数/ROI/阈值/并行)。需 feature `match-corr`。
    #[cfg(feature = "match-corr")]
    CorrWith(CorrConfig),
}

/// 组装好的找图器。内部**复用一个 [`Frame`]**,逐帧查找不再重复分配像素缓冲。
pub struct Finder {
    capture: Box<dyn Capture>,
    matcher: Box<dyn Matcher>,
    region: Option<Rect>,
    frame: Frame,
    prev_frame: Option<Frame>,
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
            prev_frame: None,
            cache_key: None,
            cache_result: None,
        }
    }

    /// 截一屏并查找模板(若设了 `region` 则只在该区域内找)。
    ///
    /// 当后端报告画面自上次以来**未变化**、且模板与区域都和上次相同,则直接复用
    /// 上次结果、跳过搜索(静态桌面轮询的常见加速;结果与重新搜一遍完全一致)。
    pub fn find_on_screen(&mut self, tpl: &Template) -> Result<Option<Match>> {
        let _t0 = px_timer!();
        let (changed, origin) = self.grab_scoped()?;
        let key = self.key_of(tpl);
        if !changed && self.cache_key == Some(key) {
            if let Some(prev) = self.cache_result {
                px_trace!(
                    op = "find_on_screen",
                    backend = self.capture.backend(),
                    cache_hit = true,
                    hit = prev.is_some(),
                    elapsed_us = _t0.map(|t| t.elapsed().as_micros() as u64).unwrap_or(0),
                );
                return Ok(prev);
            }
        }
        let m = self.search(tpl, origin);
        self.cache_key = Some(key);
        self.cache_result = Some(m);
        px_trace!(
            op = "find_on_screen",
            backend = self.capture.backend(),
            cache_hit = false,
            changed,
            frame = format!("{}x{}", self.frame.width, self.frame.height),
            tpl_key = format!("{:016x}", key),
            hit = m.is_some(),
            elapsed_us = _t0.map(|t| t.elapsed().as_micros() as u64).unwrap_or(0),
        );
        Ok(m)
    }

    /// 截一屏并按**颜色范围**找连通色块(不需要模板):`spec.tolerance` 为每
    /// 通道最大绝对差,`min_area` 过滤碎点;返回按 (y, x) 排序的 [`ColorBlob`]。
    /// 血条 / 状态灯 / 高亮区这类"只有颜色、没有模板"的场景用它。
    pub fn find_color_on_screen(
        &mut self,
        spec: &ColorSpec,
        min_area: usize,
    ) -> Result<Vec<ColorBlob>> {
        let _t0 = px_timer!();
        let (_, origin) = self.grab_scoped()?;
        let region = self.search_rect(origin);
        let mut blobs = color::find_blobs(&self.frame, spec, region, min_area);
        if origin != (0, 0) {
            // 区域帧里的坐标是相对区域左上角的,换回屏幕绝对坐标
            for b in &mut blobs {
                b.bounds.x += origin.0 as usize;
                b.bounds.y += origin.1 as usize;
            }
        }
        px_trace!(
            op = "find_color_on_screen",
            backend = self.capture.backend(),
            color = format!("{:?}±{}", spec.rgb, spec.tolerance),
            region = format!(
                "({},{},{}x{})",
                region.x, region.y, region.width, region.height
            ),
            blobs = blobs.len(),
            elapsed_us = _t0.map(|t| t.elapsed().as_micros() as u64).unwrap_or(0),
        );
        Ok(blobs)
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
                px_trace!(op = "find_until", result = "hit", timeout = ?timeout);
                return Ok(Some(m));
            }
            let now = Instant::now();
            if now >= deadline {
                px_trace!(op = "find_until", result = "timeout", timeout = ?timeout);
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
                px_trace!(op = "wait_gone", result = "gone", timeout = ?timeout);
                return Ok(true);
            }
            let now = Instant::now();
            if now >= deadline {
                px_trace!(op = "wait_gone", result = "still_present", timeout = ?timeout);
                return Ok(false);
            }
            std::thread::sleep(interval.min(deadline - now));
        }
    }

    /// 截一屏并找全部不重叠匹配(至多 `max` 个,`max=0` 不限)。
    pub fn find_all_on_screen(&mut self, tpl: &Template, max: usize) -> Result<Vec<Match>> {
        let (_, origin) = self.grab_scoped()?;
        let region = self.search_rect(origin);
        let mut ms = self.matcher.find_all(&self.frame, tpl, region, max);
        if origin != (0, 0) {
            for m in &mut ms {
                m.x += origin.0;
                m.y += origin.1;
            }
        }
        Ok(ms)
    }

    /// 只截一屏,依次匹配多个模板(省掉重复截图)。
    pub fn find_many_on_screen(&mut self, tpls: &[&Template]) -> Result<Vec<Option<Match>>> {
        let (_, origin) = self.grab_scoped()?;
        Ok(tpls.iter().map(|t| self.search(t, origin)).collect())
    }

    /// 在给定帧里查找模板(不涉及截图,便于测试/离线;遵循已设 region)。
    pub fn find_in_frame(&self, frame: &Frame, tpl: &Template) -> Option<Match> {
        match self.region {
            Some(r) => self.matcher.find_in(frame, tpl, r),
            None => self.matcher.find(frame, tpl),
        }
    }

    /// 截一屏找模板,返回**模板中心**的屏幕坐标(省去手动加半尺寸)。
    ///
    /// 命中时 `Match.x`/`Match.y` = 左上角 + 宽高的一半(整数除法)。
    /// 适合"找到后直接点击中心"的场景。
    pub fn find_center_on_screen(&mut self, tpl: &Template) -> Result<Option<Match>> {
        let m = self.find_on_screen(tpl)?;
        Ok(m.map(|m| Match {
            x: m.x + tpl.width as i32 / 2,
            y: m.y + tpl.height as i32 / 2,
            score: m.score,
        }))
    }

    /// 截一屏,与上一次截图的帧在指定区域内逐像素对比,
    /// 返回**颜色有差异的像素数**。首次调用(无参考帧)返回区域内全部像素数。
    ///
    /// 用途:"这块区域变了吗?""动画是否还在跑?""有没有新消息图标闪了一下"。
    /// 比较时使用 R/G/B 三通道(忽略 Alpha),任一通道差值 !=0 即计为不同。
    ///
    /// 注意:两次截图之间**帧布局发生变化**(切换过 `region`、显示器分辨率改变、
    /// 换了后端)时,像素不再一一对应,此时保守返回区域内全部像素数(=整片都变),
    /// 并把当前帧立为新基线;下一次调用起恢复正常计数。
    ///
    /// 成本:为供下次对比,这里会保留**当前整帧**的一份副本(1080p 约 8 MB),
    /// 属于"每调用一次拷一份"的量级,不适合 60 fps 级高频轮询。
    pub fn diff_since_last(&mut self, rect: Rect) -> Result<u32> {
        let _ = self.grab_scoped()?;
        let r = self.frame.clamp(rect);
        let count = match self.prev_frame.as_ref() {
            // 两帧布局一致(尺寸 + 像素格式)时,下标才对应同一个屏幕位置,可逐像素比。
            Some(prev)
                if prev.width == self.frame.width
                    && prev.height == self.frame.height
                    && prev.format == self.frame.format =>
            {
                let (ro, go, bo) = self.frame.rgb_offsets();
                let (pro, pgo, pbo) = prev.rgb_offsets();
                let sw4 = self.frame.width * 4;
                let pw4 = prev.width * 4;
                let mut diff = 0u32;
                for y in r.y..r.y + r.height {
                    for x in r.x..r.x + r.width {
                        let ci = y * sw4 + x * 4;
                        let pi = y * pw4 + x * 4;
                        if self.frame.pixels[ci + ro] != prev.pixels[pi + pro]
                            || self.frame.pixels[ci + go] != prev.pixels[pi + pgo]
                            || self.frame.pixels[ci + bo] != prev.pixels[pi + pbo]
                        {
                            diff += 1;
                        }
                    }
                }
                diff
            }
            // 无基线,或布局变了(切换过 region、分辨率改变、换了后端):`r` 是按
            // 当前帧裁剪的,拿它去索引更小的上一帧会越界 panic,同下标也不再指向
            // 同一屏幕位置。保守视为"区域内全部像素都变化",并把当前帧立为新基线。
            _ => (r.width * r.height) as u32,
        };
        // 当前帧变为下次比较的基线
        self.prev_frame = Some(self.frame.clone());
        Ok(count)
    }

    /// 当前限定区域(若有)。
    pub fn region(&self) -> Option<Rect> {
        self.region
    }

    /// 抓一帧供本次查找使用,返回(画面相对上次是否变化, 该帧左上角的屏幕坐标)。
    ///
    /// 设了 region 且后端支持直接区域抓取([`Capture::grab_region`] 返回
    /// `Some`)时只截该区域——省掉一次整屏拷贝与后续裁剪。此时帧内坐标是**区域
    /// 相对**的,第二个返回值就是需要加回的偏移;不支持则为 `(changed, (0, 0))`,
    /// 行为与旧版"截全屏再按 region 裁剪"完全一致。
    fn grab_scoped(&mut self) -> Result<(bool, (i32, i32))> {
        if let Some(r) = self.region {
            if let Some(f) = self.capture.grab_region(r)? {
                self.frame = f;
                return Ok((true, (r.x as i32, r.y as i32)));
            }
        }
        Ok((self.capture.grab_into(&mut self.frame)?, (0, 0)))
    }

    /// 本次搜索应在帧内哪个矩形上进行。
    fn search_rect(&self, origin: (i32, i32)) -> Rect {
        if origin == (0, 0) {
            self.region.unwrap_or_else(|| self.frame.full_rect())
        } else {
            // 帧本身就是 region,整帧搜索即等价于旧的"全屏 + region 裁剪"
            self.frame.full_rect()
        }
    }

    /// 在 [`grab_scoped`](Self::grab_scoped) 产出的帧上查模板,结果换算为屏幕绝对坐标。
    ///
    /// 能走 [`Matcher::find`] 就走:`find_in` 的**默认实现**是"裁剪子帧 + 拷贝",
    /// 没覆写它的匹配器(如 `CorrMatcher`)在整屏帧上会白拷一帧(1080p 约 8MB)。
    /// 只有"全屏帧 + 限定区域"才真的需要 `find_in` 收窄范围。
    fn search(&self, tpl: &Template, origin: (i32, i32)) -> Option<Match> {
        let mut m = if origin != (0, 0) {
            // 帧本身就是 region,整帧搜即等价于旧的"全屏 + region 裁剪"
            self.matcher.find(&self.frame, tpl)?
        } else {
            match self.region {
                Some(r) => self.matcher.find_in(&self.frame, tpl, r)?,
                None => self.matcher.find(&self.frame, tpl)?,
            }
        };
        m.x += origin.0;
        m.y += origin.1;
        Some(m)
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
        let capture: Box<dyn Capture> = match self.capture.unwrap_or(CaptureKind::Monitor) {
            CaptureKind::Monitor => Box::new(XCapCapture::primary()?),
            #[cfg(all(windows, feature = "capture-gdi"))]
            CaptureKind::Gdi => Box::new(crate::capture_gdi::GdiCapture::new_primary()),
            #[cfg(all(windows, feature = "capture-dxgi"))]
            CaptureKind::Dxgi => Box::new(
                crate::capture_dxgi::DxgiCapture::new_primary()
                    .ok_or_else(|| Error::capture("DXGI desktop duplication unavailable"))?,
            ),
            #[cfg(all(windows, feature = "capture-window"))]
            CaptureKind::Window(h) => Box::new(crate::capture_window::WindowCapture::new(h)),
            #[cfg(all(windows, feature = "capture-window"))]
            CaptureKind::WindowByTitle(ref title) => {
                Box::new(crate::capture_window::WindowCapture::from_title(title)?)
            }
            #[cfg(all(windows, any(feature = "capture-gdi", feature = "capture-dxgi")))]
            CaptureKind::Auto => auto_capture()?,
        };
        let matcher: Box<dyn Matcher> =
            match self.matcher.unwrap_or(MatchKind::Rgb { tolerance: 25 }) {
                MatchKind::Rgb { tolerance } => Box::new(RgbMatcher::new(tolerance)),
                #[cfg(feature = "match-corr")]
                MatchKind::Corr => Box::new(CorrMatcher::new()),
                #[cfg(feature = "match-corr")]
                MatchKind::CorrWith(cfg) => Box::new(CorrMatcher::with_config(cfg)),
            };
        let mut f = Finder::new(capture, matcher);
        f.region = self.region;
        Ok(f)
    }
}

/// 依次尝试 DXGI → GDI → xcap,返回第一个可用的。
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
    Ok(Box::new(XCapCapture::primary()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::Cell;
    use std::rc::Rc;

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

        fn backend(&self) -> &'static str {
            "mock"
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

    #[test]
    fn find_color_on_screen_high_level_entry() {
        let mut f = finder_with(vec![true]);
        let blobs = f
            .find_color_on_screen(&ColorSpec::new(255, 0, 0, 10), 32)
            .unwrap();
        assert_eq!(blobs.len(), 1);
        assert_eq!(blobs[0].bounds, Rect::new(10, 10, 8, 8));
        assert_eq!(blobs[0].area, 64);
        // region 限定时,区域外的色块不可见
        f.set_region(Some(Rect::new(0, 20, 32, 12)));
        assert!(f
            .find_color_on_screen(&ColorSpec::new(255, 0, 0, 10), 1)
            .unwrap()
            .is_empty());
    }

    /// 区域帧:24x24,一块 8x8 纯红贴在帧内 (4,5)。
    fn region_frame() -> Frame {
        let (w, h) = (24usize, 24usize);
        let mut px = vec![0u8; w * h * 4];
        for y in 5..13 {
            for x in 4..12 {
                let i = (y * w + x) * 4;
                px[i] = 255;
                px[i + 3] = 255;
            }
        }
        Frame::rgba8(w, h, px)
    }

    /// 支持区域截取的后端:全屏帧里故意没有目标,只有区域帧里有。
    struct RegionCap {
        region_grabs: Rc<Cell<usize>>,
        full_grabs: Rc<Cell<usize>>,
    }

    impl Capture for RegionCap {
        fn grab(&mut self) -> Result<Frame> {
            self.full_grabs.set(self.full_grabs.get() + 1);
            Ok(frame(false))
        }

        fn grab_region(&mut self, r: Rect) -> Result<Option<Frame>> {
            assert_eq!(
                (r.x, r.y, r.width, r.height),
                (20, 16, 24, 24),
                "应收到屏幕绝对坐标区域"
            );
            self.region_grabs.set(self.region_grabs.get() + 1);
            Ok(Some(region_frame()))
        }

        fn backend(&self) -> &'static str {
            "mock-region"
        }
    }

    fn region_finder() -> (Finder, Rc<Cell<usize>>, Rc<Cell<usize>>) {
        let (rg, fg) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let mut f = Finder::new(
            Box::new(RegionCap {
                region_grabs: rg.clone(),
                full_grabs: fg.clone(),
            }),
            Box::new(RgbMatcher::new(0)),
        );
        f.set_region(Some(Rect::new(20, 16, 24, 24)));
        (f, rg, fg)
    }

    /// region + 支持区域截取的后端:只截区域,且结果换算回屏幕绝对坐标。
    #[test]
    fn region_prefers_backend_region_grab() {
        let (mut f, rg, fg) = region_finder();
        let m = f
            .find_on_screen(&target_tpl())
            .unwrap()
            .expect("区域内应命中");
        // 帧内 (4,5) + 区域原点 (20,16)
        assert_eq!((m.x, m.y), (24, 21), "应返回屏幕绝对坐标");
        assert_eq!(rg.get(), 1);
        assert_eq!(fg.get(), 0, "能只截区域时不应再截全屏");
    }

    /// find_all / find_color 两条入口在区域截图下也必须给出绝对坐标。
    #[test]
    fn region_grab_shifts_all_and_color_results() {
        let (mut f, rg, _) = region_finder();

        let all = f.find_all_on_screen(&target_tpl(), 0).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!((all[0].x, all[0].y), (24, 21));

        let blobs = f
            .find_color_on_screen(&ColorSpec::new(255, 0, 0, 10), 32)
            .unwrap();
        assert_eq!(blobs.len(), 1);
        assert_eq!(blobs[0].bounds, Rect::new(24, 21, 8, 8));
        assert_eq!(blobs[0].area, 64);
        assert_eq!(rg.get(), 2, "两次入口都应走区域路径");
    }

    /// 只记录走了哪个入口的匹配器(没覆写 `find_in` 的匹配器走它会整帧拷贝)。
    struct SpyMatcher {
        finds: Rc<Cell<usize>>,
        find_ins: Rc<Cell<usize>>,
    }

    impl Matcher for SpyMatcher {
        fn find(&self, _frame: &Frame, _tpl: &Template) -> Option<Match> {
            self.finds.set(self.finds.get() + 1);
            Some(Match {
                x: 1,
                y: 2,
                score: 1.0,
            })
        }

        fn find_in(&self, _frame: &Frame, _tpl: &Template, _region: Rect) -> Option<Match> {
            self.find_ins.set(self.find_ins.get() + 1);
            Some(Match {
                x: 1,
                y: 2,
                score: 1.0,
            })
        }
    }

    fn spy() -> (SpyMatcher, Rc<Cell<usize>>, Rc<Cell<usize>>) {
        let (f, i) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        (
            SpyMatcher {
                finds: f.clone(),
                find_ins: i.clone(),
            },
            f,
            i,
        )
    }

    /// 未设 region 时必须走 `find`:走 `find_in` 会让没覆写它的匹配器(如
    /// `CorrMatcher`)白拷一整帧。
    #[test]
    fn no_region_uses_find_not_find_in() {
        let (m, finds, find_ins) = spy();
        let mut f = Finder::new(
            Box::new(SeqCapture {
                present: vec![true],
                grabs: 0,
            }),
            Box::new(m),
        );
        assert!(f.find_on_screen(&target_tpl()).unwrap().is_some());
        assert_eq!((finds.get(), find_ins.get()), (1, 0));
    }

    /// 全屏帧 + 限定区域(后端不支持区域直抓):必须用 `find_in` 收窄范围。
    #[test]
    fn region_without_backend_support_uses_find_in() {
        let (m, finds, find_ins) = spy();
        let mut f = Finder::new(
            Box::new(SeqCapture {
                present: vec![true],
                grabs: 0,
            }),
            Box::new(m),
        );
        f.set_region(Some(Rect::new(2, 3, 8, 8)));
        assert!(f.find_on_screen(&target_tpl()).unwrap().is_some());
        assert_eq!((finds.get(), find_ins.get()), (0, 1));
    }

    /// 区域直抓时帧本身就是 region,再走 `find_in(整帧)` 就是多余的一次拷贝;
    /// 同时坐标仍要换算回屏幕绝对位置。
    #[test]
    fn region_grab_searches_whole_frame_with_find() {
        let (m, finds, find_ins) = spy();
        let mut f = Finder::new(
            Box::new(RegionCap {
                region_grabs: Rc::new(Cell::new(0)),
                full_grabs: Rc::new(Cell::new(0)),
            }),
            Box::new(m),
        );
        f.set_region(Some(Rect::new(20, 16, 24, 24)));
        let hit = f.find_on_screen(&target_tpl()).unwrap().unwrap();
        // 帧内 (1,2) + 区域原点 (20,16)
        assert_eq!((hit.x, hit.y), (21, 18));
        assert_eq!((finds.get(), find_ins.get()), (1, 0));
    }

    #[test]
    fn find_center_returns_center_coords() {
        // 目标 8x8 贴在 (10,10),中心 = (14, 14)
        let mut f = finder_with(vec![true]);
        let m = f
            .find_center_on_screen(&target_tpl())
            .unwrap()
            .expect("应命中");
        assert_eq!((m.x, m.y), (10 + 8 / 2, 10 + 8 / 2));
    }

    #[test]
    fn diff_since_last_first_call_returns_area() {
        // 首次无基线,应返回全部像素数
        let mut f = finder_with(vec![true]);
        let d = f.diff_since_last(Rect::new(0, 0, 10, 10)).unwrap();
        assert_eq!(d, 100);
    }

    #[test]
    fn diff_since_last_same_frame_returns_zero() {
        // 同一帧连续两次 diff:无变化应为 0
        let mut f = finder_with(vec![true]);
        let _ = f.diff_since_last(Rect::new(0, 0, 32, 32)).unwrap();
        let d = f.diff_since_last(Rect::new(0, 0, 32, 32)).unwrap();
        assert_eq!(d, 0);
    }

    #[test]
    fn diff_since_last_detects_change() {
        // 第一帧无目标(全黑),第二帧有目标(红色块)→ diff > 0
        let mut f = finder_with(vec![false, true]);
        let _ = f.diff_since_last(Rect::new(0, 0, 32, 32)).unwrap();
        let d = f.diff_since_last(Rect::new(0, 0, 32, 32)).unwrap();
        assert!(d > 0, "两帧不同应检出变化,got {}", d);
    }

    /// 支持 `grab_region`、按请求尺寸给帧的 mock(真实后端切换区域时帧尺寸会变)。
    struct RegionCapture;
    impl Capture for RegionCapture {
        fn grab(&mut self) -> Result<Frame> {
            Ok(Frame::bgra8(64, 64, vec![9u8; 64 * 64 * 4]))
        }
        fn grab_region(&mut self, r: Rect) -> Result<Option<Frame>> {
            Ok(Some(Frame::bgra8(
                r.width,
                r.height,
                vec![9u8; r.width * r.height * 4],
            )))
        }
    }

    #[test]
    fn diff_since_last_survives_region_resize() {
        // 回归:先在 8x8 小区域建基线(上一帧仅 256 像素),再切到 64x64 大区域,
        // 用大帧的下标去索引小帧缓冲会越界 panic。
        let mut f = Finder::new(Box::new(RegionCapture), Box::new(RgbMatcher::new(0)));
        f.set_region(Some(Rect::new(0, 0, 8, 8)));
        let _ = f.diff_since_last(Rect::new(0, 0, 8, 8)).unwrap();

        f.set_region(Some(Rect::new(0, 0, 64, 64)));
        let d = f.diff_since_last(Rect::new(0, 0, 16, 16)).unwrap();
        assert_eq!(d, 16 * 16, "两帧布局不同应保守报告整片变化");

        // 基线已重建为同一布局,内容未变时应回到 0
        let d2 = f.diff_since_last(Rect::new(0, 0, 16, 16)).unwrap();
        assert_eq!(d2, 0);
    }
}
