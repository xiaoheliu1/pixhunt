//! 基于 `corrmatch` 的高鲁棒匹配器:ZNCC + 金字塔,抗光照/轻微缩放变化。
//!
//! 该匹配器工作在**灰度**上:每帧把 [`Frame`] 与 [`Template`] 各转一次灰度后交给
//! corrmatch。比 [`crate::RgbMatcher`] 慢,但在内容有变化时更稳。

use crate::frame::Frame;
use crate::matcher::{Match, Matcher};
use crate::template::Template;

use corrmatch::{
    CompileConfig, ImageView, MatchConfig, RotationMode, Template as CorrTemplate,
    Matcher as CorrInner,
};

/// ZNCC 匹配器。
pub struct CorrMatcher;

impl CorrMatcher {
    pub fn new() -> Self {
        CorrMatcher
    }
}

impl Default for CorrMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Matcher for CorrMatcher {
    fn find(&self, frame: &Frame, tpl: &Template) -> Option<Match> {
        // 1) 模板灰度 -> corrmatch Template -> compile(建金字塔)
        let corr_tpl = CorrTemplate::new(tpl.to_gray(), tpl.width, tpl.height).ok()?;
        let compiled = corr_tpl.compile(CompileConfig::default()).ok()?;
        // 只做平移匹配,关闭旋转搜索
        let inner = CorrInner::new(compiled).with_config(MatchConfig {
            rotation: RotationMode::Disabled,
            ..MatchConfig::default()
        });

        // 2) 帧灰度 -> ImageView -> 匹配
        let gray = frame.to_gray();
        let view = ImageView::from_slice(&gray, frame.width, frame.height).ok()?;
        let res = inner.match_image(view).ok()?;

        Some(Match { x: res.x as i32, y: res.y as i32, score: res.score })
    }
}
