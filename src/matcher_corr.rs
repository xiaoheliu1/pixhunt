//! 基于 `corrmatch` 的高鲁棒匹配器:ZNCC + 金字塔,抗光照/轻微缩放变化。
//!
//! 工作在**灰度**上:把 [`Frame`] 与 [`Template`] 各转一次灰度后交给 corrmatch。
//! 编译后的模板(金字塔)按 [`Template::content_key`] 缓存在内部,同一模板反复查找
//! 不必重编译;灰度 scratch 亦复用,避免每帧分配。比 [`crate::RgbMatcher`] 慢但更稳。
//!
//! 注意:ZNCC 依赖局部对比度,**纯色/零方差模板**在该度量下无意义,匹配不到属预期。

use std::cell::RefCell;

use crate::frame::Frame;
use crate::matcher::{Match, Matcher};
use crate::template::Template;

use corrmatch::{
    CompileConfig, ImageView, MatchConfig, Matcher as CorrInner, RotationMode,
    Template as CorrTemplate,
};

struct Cached {
    key: u64,
    inner: CorrInner,
}

/// ZNCC 匹配器(内部缓存已编译模板 + 复用的灰度缓冲)。
pub struct CorrMatcher {
    cache: RefCell<Option<Cached>>,
    gray: RefCell<Vec<u8>>,
}

impl CorrMatcher {
    pub fn new() -> Self {
        CorrMatcher {
            cache: RefCell::new(None),
            gray: RefCell::new(Vec::new()),
        }
    }

    /// 确保缓存中的编译模板对应当前 `tpl`(内容变了才重编译)。
    fn ensure_compiled(&self, tpl: &Template) -> Option<()> {
        let key = tpl.content_key();
        let mut cache = self.cache.borrow_mut();
        if cache.as_ref().map(|c| c.key) == Some(key) {
            return Some(());
        }
        let corr_tpl = CorrTemplate::new(tpl.to_gray(), tpl.width, tpl.height).ok()?;
        let compiled = corr_tpl.compile(CompileConfig::default()).ok()?;
        // 只做平移匹配,关闭旋转搜索
        let inner = CorrInner::new(compiled).with_config(MatchConfig {
            rotation: RotationMode::Disabled,
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
        self.ensure_compiled(tpl)?;

        // 帧灰度 -> 复用的 scratch 缓冲
        frame.to_gray_into(&mut self.gray.borrow_mut());

        let cache = self.cache.borrow();
        let gray = self.gray.borrow();
        let inner = &cache.as_ref()?.inner;
        let view = ImageView::from_slice(&gray, frame.width, frame.height).ok()?;
        let res = inner.match_image(view).ok()?;
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
}
