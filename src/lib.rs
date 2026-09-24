//! # pixhunt
//!
//! 快速、低依赖的**屏幕找图**库:截一帧屏幕,在其中定位一张小图(模板)的坐标。
//!
//! 两个"可插拔插座":
//! - [`Capture`]  —— 怎么拿到画面:`ScreenshotsCapture`(跨平台保底),
//!   以及 Windows 上更快的 `GdiCapture` / `DxgiCapture`(feature 门控)。
//! - [`Matcher`]  —— 怎么找模板:内置极速 [`RgbMatcher`],以及基于 corrmatch 的
//!   高鲁棒 ZNCC `CorrMatcher`(feature `match-corr`)。
//!
//! 快速上手见 `examples/find_on_screen.rs` 与 README。
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

pub mod capture;
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

#[cfg(feature = "match-corr")]
mod matcher_corr;
#[cfg(feature = "match-corr")]
pub use matcher_corr::CorrMatcher;

pub use capture::{Capture, ScreenshotsCapture};
pub use error::{Error, Result};
pub use finder::{CaptureKind, Finder, MatchKind};
pub use frame::{Frame, PixelFormat};
pub use matcher::{Match, Matcher, RgbMatcher};
pub use template::Template;
