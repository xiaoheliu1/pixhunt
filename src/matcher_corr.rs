//! 基于 `corrmatch` 的高鲁棒匹配器:ZNCC + 金字塔,抗光照/轻微缩放变化。
//!
//! 工作在**灰度**上:把 [`Frame`] 与 [`Template`] 各转一次灰度后交给 corrmatch。
//! 只做**平移匹配**:模板走 `compile_unrotated`,不构建角度模板库。
//! 编译后的模板(金字塔)按 [`Template::content_key`] 缓存在内部,同一模板反复查找
//! 不必重编译;灰度 scratch 亦复用,避免每帧分配。比 [`crate::RgbMatcher`] 慢但更稳。
//! 延迟敏感的场合用 [`CorrMatcher::with_config`] 调 [`CorrConfig`];feature `parallel`
//! 打开时走 corrmatch 的分层并行搜索(结果仍为确定性)。
//!
//! 注意:ZNCC 依赖局部对比度,**纯色/零方差模板**在该度量下无意义,匹配不到属预期。

use std::cell::RefCell;

use crate::frame::Frame;
use crate::matcher::{Match, Matcher};
use crate::template::Template;

use corrmatch::{
    CompileConfigNoRot, CompiledTemplate, ImageView, MatchConfig, Matcher as CorrInner,
    RotationMode, Template as CorrTemplate,
};

struct Cached {
    key: u64,
    inner: CorrInner,
}

/// 金字塔层数:建到最小边 ~4px 为止(上限 6 层)。
///
/// 与 `compile_rotated` 的行为对齐:rotated 路径会把 `min_dim < 3` 的退化层剔除,
/// 而 `CompileConfigNoRot::default()` 的 `max_levels=6` 会多留一层(如 64px 模板
/// 降到 2x2)。2x2 模板在粗筛层的 ZNCC 几乎没有区分度,容易选错种子。
fn no_rot_levels(w: usize, h: usize) -> usize {
    let mut levels = 0usize;
    let (mut cw, mut ch) = (w, h);
    while levels < 6 && cw.min(ch) >= 4 {
        levels += 1;
        cw /= 2;
        ch /= 2;
    }
    levels.max(1)
}

/// [`CorrMatcher`] 的搜索调参。只暴露对**延迟/命中质量**影响最大、且取值安全的
/// 几个旋钮;其余保持 corrmatch 默认。
///
/// 所有字段都会被夹到合法下限(而非报错),因此不会出现"配了个非法值就永远找
/// 不到"的静默失败。
/// ```
/// use pixhunt::{CorrConfig, CorrMatcher};
///
/// // 已知目标只在附近小范围移动:砍掉深层金字塔 + 缩小精修 ROI,换延迟
/// let m = CorrMatcher::with_config(CorrConfig {
///     max_image_levels: 3,
///     roi_radius: 4,
///     min_score: 0.7,
///     ..CorrConfig::default()
/// });
/// ```
#[derive(Clone, Debug)]
pub struct CorrConfig {
    /// 图像金字塔层数上限(默认 6)。调小 → 粗筛更便宜,但大位移/强缩放场景更易漏。
    pub max_image_levels: usize,
    /// 每层保留的候选束宽(默认 8)。调大更稳但更慢,最小 1。
    pub beam_width: usize,
    /// 逐层精修的 ROI 半径(默认 8)。已知目标位移很小时可调小提速。
    pub roi_radius: usize,
    /// **最终结果**的最低分门槛(默认不过滤)。低于此分视为未命中,`score` 取值
    /// `0.0..=1.0`(ZNCC 相似度)。
    ///
    /// 在 pixhunt 侧过滤,不会传给 corrmatch——后者的 `min_score` 是**逐金字塔层**的
    /// 候选门槛,而粗筛层的 ZNCC 分数天然偏低(降采样吃掉了对比度),拿它当最终阈值
    /// 会把真命中整条链路削空。
    pub min_score: f32,
    /// 分层并行搜索(默认跟随 feature `parallel`)。
    ///
    /// 打开需要 corrmatch 的 `rayon` feature,pixhunt 的 `parallel` feature 已代为传导;
    /// 没开该 feature 时这里会被归一为 false。
    pub parallel: bool,
}

impl Default for CorrConfig {
    fn default() -> Self {
        CorrConfig {
            max_image_levels: 6,
            beam_width: 8,
            roi_radius: 8,
            min_score: f32::NEG_INFINITY,
            parallel: cfg!(feature = "parallel"),
        }
    }
}

impl CorrConfig {
    /// 把非法值夹到安全范围,保证下游 `MatchConfig::validate()` 永不会失败。
    fn sanitize(&self) -> CorrConfig {
        CorrConfig {
            max_image_levels: self.max_image_levels.max(1),
            beam_width: self.beam_width.max(1),
            roi_radius: self.roi_radius.max(1),
            // NaN 会污染比较,归一为不限制;正无穷钳到 f32::MAX(任何结果都不达标),
            // 负无穷保持"不过滤"。
            min_score: match self.min_score {
                v if v.is_nan() || v == f32::NEG_INFINITY => f32::NEG_INFINITY,
                v if v == f32::INFINITY => f32::MAX,
                v => v,
            },
            // pixhunt 的 `parallel` feature 才会给 corrmatch 传导 rayon;未开该 feature 时
            // 强行置 true 会让下游 validate 报 ParallelUnavailable,find 静默返回 None。
            parallel: self.parallel && cfg!(feature = "parallel"),
        }
    }
}

/// ZNCC 匹配器(内部缓存已编译模板 + 复用的灰度缓冲)。
pub struct CorrMatcher {
    cfg: CorrConfig,
    cache: RefCell<Option<Cached>>,
    gray: RefCell<Vec<u8>>,
}

impl CorrMatcher {
    /// 默认配置(纯平移 + corrmatch 默认搜索参数)。
    pub fn new() -> Self {
        Self::with_config(CorrConfig::default())
    }

    /// 自定义搜索参数。非法值会被夹到安全下限,不会导致查找静默失败。
    pub fn with_config(cfg: CorrConfig) -> Self {
        CorrMatcher {
            cfg: cfg.sanitize(),
            cache: RefCell::new(None),
            gray: RefCell::new(Vec::new()),
        }
    }

    /// 当前生效的配置。
    pub fn config(&self) -> &CorrConfig {
        &self.cfg
    }

    /// 确保缓存中的编译模板对应当前 `tpl`(内容变了才重编译)。
    fn ensure_compiled(&self, tpl: &Template) -> Option<()> {
        let key = tpl.content_key();
        let mut cache = self.cache.borrow_mut();
        if cache.as_ref().map(|c| c.key) == Some(key) {
            return Some(());
        }
        let corr_tpl = CorrTemplate::new(tpl.to_gray(), tpl.width, tpl.height).ok()?;
        // 只做平移匹配:走 compile_unrotated,不构建永远用不到的角度模板库;
        // 匹配侧仍显式关闭旋转搜索,与编译产物保持一致。
        let compile_cfg = CompileConfigNoRot {
            max_levels: no_rot_levels(tpl.width, tpl.height),
        };
        let compiled = CompiledTemplate::compile_unrotated(&corr_tpl, compile_cfg).ok()?;
        let inner = CorrInner::new(compiled).with_config(MatchConfig {
            rotation: RotationMode::Disabled,
            // 已经 `sanitize` 过,下游 `validate()` 不会再报错。
            parallel: self.cfg.parallel,
            max_image_levels: self.cfg.max_image_levels,
            beam_width: self.cfg.beam_width,
            roi_radius: self.cfg.roi_radius,
            ..MatchConfig::default()
        });
        *cache = Some(Cached { key, inner });
        Some(())
    }
}

impl Default for CorrMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Matcher for CorrMatcher {
    fn find(&self, frame: &Frame, tpl: &Template) -> Option<Match> {
        // ZNCC 是整块灰度相关,corrmatch 没有掩码接口:带掩码的模板里被掩掉的
        // 像素(透明区)仍会连同其 RGB 参与打分,结果可能与 RgbMatcher 不一致。
        // 不静默——debug 构建下明确提示,release 构建零开销。
        debug_assert!(
            tpl.mask.is_none(),
            "CorrMatcher 不支持带掩码的模板:透明区域会参与 ZNCC 打分,请改用 RgbMatcher"
        );
        self.ensure_compiled(tpl)?;

        // 帧灰度 -> 复用的 scratch 缓冲
        frame.to_gray_into(&mut self.gray.borrow_mut());

        let cache = self.cache.borrow();
        let gray = self.gray.borrow();
        let inner = &cache.as_ref()?.inner;
        let view = ImageView::from_slice(&gray, frame.width, frame.height).ok()?;
        let res = inner.match_image(view).ok()?;
        // 最终阈值在 pixhunt 侧把关(见 `CorrConfig::min_score`)。
        if res.score < self.cfg.min_score {
            return None;
        }
        Some(Match {
            x: res.x as i32,
            y: res.y as i32,
            score: res.score,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Rect;

    /// 一张有起伏的背景(保证局部对比度,ZNCC 才有意义)。
    fn noisy_bg(w: usize, h: usize) -> Vec<u8> {
        let mut px = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                px[i] = ((x * 7 + y * 13) % 256) as u8;
                px[i + 1] = ((x * 3 + y * 11) % 256) as u8;
                px[i + 2] = ((x * 5 + y * 17) % 256) as u8;
                px[i + 3] = 255;
            }
        }
        px
    }

    /// 在 (tx,ty) 贴一块高对比、有纹理的 patch,返回其 RGB 字节。
    fn paste_textured(px: &mut [u8], w: usize, tx: usize, ty: usize, s: usize) -> Vec<u8> {
        let mut rgb = Vec::with_capacity(s * s * 3);
        for yy in 0..s {
            for xx in 0..s {
                let i = ((ty + yy) * w + (tx + xx)) * 4;
                // 棋盘状强对比 + 渐变,保证模板方差且无短周期自相似
                let checker = if (xx / 4 + yy / 4) % 2 == 0 { 240 } else { 15 };
                let r = (checker - xx as i32 * 2).clamp(0, 255) as u8;
                let g = (checker - yy as i32 * 2).clamp(0, 255) as u8;
                let b = ((xx * 5 + yy * 3) % 256) as u8;
                px[i] = r;
                px[i + 1] = g;
                px[i + 2] = b;
                rgb.extend_from_slice(&[r, g, b]);
            }
        }
        rgb
    }

    /// 同一模板查两次:第二次走缓存编译路径,结果应与第一次一致。
    #[test]
    fn cached_compile_is_consistent() {
        let (w, h) = (256usize, 256usize);
        let mut px = noisy_bg(w, h);
        let rgb = paste_textured(&mut px, w, 150, 120, 32);
        let frame = Frame::rgba8(w, h, px);
        let t = Template::from_rgb(rgb, 32, 32);
        let m = CorrMatcher::new();
        let a = m.find(&frame, &t).expect("first find");
        let b = m.find(&frame, &t).expect("cached find");
        assert_eq!((a.x, a.y), (150, 120));
        assert_eq!(a.x, b.x);
        assert_eq!(a.y, b.y);
    }

    /// trait 默认的 find_in(裁剪)对 CorrMatcher 也应给出绝对坐标。
    #[test]
    fn region_default_works() {
        let (w, h) = (256usize, 256usize);
        let mut px = noisy_bg(w, h);
        let rgb = paste_textured(&mut px, w, 60, 70, 32);
        let frame = Frame::rgba8(w, h, px);
        let t = Template::from_rgb(rgb, 32, 32);
        let m = CorrMatcher::new();
        let hit = m
            .find_in(&frame, &t, Rect::new(50, 60, 64, 64))
            .expect("区域内应命中");
        assert_eq!((hit.x, hit.y), (60, 70));
    }

    /// 层数守卫:模板金字塔不能降到 2x2 这类退化层,否则粗筛 ZNCC 无区分度。
    #[test]
    fn levels_stop_at_non_degenerate_size() {
        assert_eq!(no_rot_levels(64, 64), 5);
        assert_eq!(no_rot_levels(32, 32), 4);
        assert_eq!(no_rot_levels(8, 8), 2);
        assert_eq!(no_rot_levels(4, 4), 1);
        // 过小模板仍保底 1 层
        assert_eq!(no_rot_levels(2, 2), 1);
        // 非方形按短边收敛
        assert_eq!(no_rot_levels(1000, 60), 4);
    }

    /// 大屏 + 较大模板:应命中真实位置(走无旋转的纯平移路径)。
    #[test]
    fn big_frame_textured_template_hits() {
        let (w, h) = (960usize, 540usize);
        let mut px = noisy_bg(w, h);
        let rgb = paste_textured(&mut px, w, 600, 380, 64);
        let frame = Frame::rgba8(w, h, px);
        let t = Template::from_rgb(rgb, 64, 64);
        let hit = CorrMatcher::new().find(&frame, &t).expect("全屏应命中");
        assert!(
            (hit.x - 600).abs() <= 2 && (hit.y - 380).abs() <= 2,
            "命中偏移过多: {:?}",
            (hit.x, hit.y)
        );
        assert!(hit.score > 0.9, "命中置信度过低: {}", hit.score);
    }

    /// 非法参数被夹到安全下限,而不是导致静默失败。
    #[test]
    fn invalid_config_is_clamped() {
        let c = CorrConfig {
            max_image_levels: 0,
            beam_width: 0,
            roi_radius: 0,
            min_score: f32::NAN,
            parallel: true,
        };
        let s = c.sanitize();
        assert_eq!(s.max_image_levels, 1);
        assert_eq!(s.beam_width, 1);
        assert_eq!(s.roi_radius, 1);
        assert_eq!(s.min_score, f32::NEG_INFINITY);
        // parallel 只在 feature 打开时才保留 true
        assert_eq!(s.parallel, cfg!(feature = "parallel"));
    }

    /// 夹完之后的配置仍能正常命中(验证不会把可用参数改坑)。
    #[test]
    fn clamped_config_still_finds() {
        let (w, h) = (256usize, 256usize);
        let mut px = noisy_bg(w, h);
        let rgb = paste_textured(&mut px, w, 150, 120, 32);
        let frame = Frame::rgba8(w, h, px);
        let t = Template::from_rgb(rgb, 32, 32);
        let m = CorrMatcher::with_config(CorrConfig {
            max_image_levels: 0,
            beam_width: 0,
            roi_radius: 0,
            min_score: f32::NAN,
            parallel: true,
        });
        let hit = m.find(&frame, &t).expect("非法入参被夹安全后应命中");
        assert_eq!((hit.x, hit.y), (150, 120));
    }

    /// `min_score` 门槛应生效:高于峰值时当作未命中。
    #[test]
    fn min_score_filters_weak_hits() {
        let (w, h) = (256usize, 256usize);
        let mut px = noisy_bg(w, h);
        let rgb = paste_textured(&mut px, w, 150, 120, 32);
        let frame = Frame::rgba8(w, h, px);
        let t = Template::from_rgb(rgb, 32, 32);

        let loose = CorrMatcher::with_config(CorrConfig {
            min_score: 0.5,
            ..CorrConfig::default()
        });
        assert!(loose.find(&frame, &t).is_some(), "高于 0.5 的命中应保留");

        let strict = CorrMatcher::with_config(CorrConfig {
            min_score: 1.0001,
            ..CorrConfig::default()
        });
        assert!(
            strict.find(&frame, &t).is_none(),
            "不可能的阈值应过滤掉结果"
        );
    }
}
