//! # pixhunt
//!
//! 快速、低依赖的**屏幕找图**库:截一帧屏幕,在其中定位一张小图(模板)的坐标。
//!
//! 两个"可插拔插座":
//! - [`Capture`](capture::Capture) —— 怎么拿到画面:跨平台的 `ScreenshotsCapture`,
//!   以及 Windows 上更快的 `GdiCapture` / `DxgiCapture` 与截单个窗口的
//!   `WindowCapture`(feature 门控)。
//! - [`Matcher`](matcher::Matcher) —— 怎么找模板:内置极速 [`RgbMatcher`](matcher::RgbMatcher)
//!   (支持容差),以及基于 corrmatch 的高鲁棒 ZNCC `CorrMatcher`(feature `match-corr`)。
//!
//! 常用能力:整屏查找 [`Finder::find_on_screen`]、限定区域查找
//! [`Matcher::find_in`]、多结果 [`Matcher::find_all`]、一次截图匹配多模板
//! [`Finder::find_many_on_screen`]、轮询等待出现/消失 [`Finder::find_until`] /
//! [`Finder::wait_gone`]、颜色范围搜索 [`Finder::find_color_on_screen`]。
//! 开 feature `parallel` 可用 rayon 按行并行加速;开 `tracing` 输出 trace 级
//! 诊断事件(截图耗时、缓存跳过、命中与否),排查"为什么找不到"。
//!
//! ```no_run
//! use pixhunt::{Finder, CaptureKind, MatchKind, Template};
//! let tpl = Template::load("template.png").unwrap();
//! let mut finder = Finder::builder()
//!     .capture(CaptureKind::Screenshots)
//!     .matcher(MatchKind::Rgb { tolerance: 25 })
//!     .build()
//!     .unwrap();
//! if let Some(m) = finder.find_on_screen(&tpl).unwrap() {
//!     println!("found at ({}, {})", m.x, m.y);
//! }
//! ```

/// 内部埋点宏:feature `tracing` 关闭时展开为空(连参数求值都不发生,零开销)。
#[cfg(feature = "tracing")]
macro_rules! px_trace {
    ($($t:tt)*) => { ::tracing::trace!(target: "pixhunt", $($t)*) };
}
#[cfg(not(feature = "tracing"))]
macro_rules! px_trace {
    ($($t:tt)*) => {};
}

/// 与 [`px_trace!`] 配套的诊断计时器:关闭时不取时间戳。
#[cfg(feature = "tracing")]
macro_rules! px_timer {
    () => {
        Some(std::time::Instant::now())
    };
}
#[cfg(not(feature = "tracing"))]
macro_rules! px_timer {
    () => {
        None::<std::time::Instant>
    };
}

pub mod capture;
pub mod color;
pub mod error;
pub mod finder;
pub mod frame;
pub mod matcher;
pub mod template;

#[cfg(all(windows, feature = "capture-gdi"))]
mod capture_gdi;
#[cfg(all(windows, feature = "capture-gdi"))]
pub use capture_gdi::GdiCapture;

#[cfg(all(windows, feature = "capture-dxgi"))]
mod capture_dxgi;
#[cfg(all(windows, feature = "capture-dxgi"))]
pub use capture_dxgi::DxgiCapture;

#[cfg(all(windows, feature = "capture-window"))]
mod capture_window;
#[cfg(all(windows, feature = "capture-window"))]
pub use capture_window::{WindowCapture, WindowHandle};

#[cfg(feature = "match-corr")]
mod matcher_corr;
#[cfg(feature = "match-corr")]
pub use matcher_corr::CorrMatcher;

pub use capture::{Capture, ScreenshotsCapture};
pub use color::{ColorBlob, ColorSpec, FindColor};
pub use error::{Error, Result};
pub use finder::{CaptureKind, Finder, MatchKind};
pub use frame::{Frame, PixelFormat, Rect};
pub use matcher::{Match, Matcher, RgbMatcher};
pub use template::Template;
