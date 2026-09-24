//! 统一的错误类型与 `Result` 别名。
use std::fmt;

/// pixhunt 的错误。
#[derive(Debug)]
pub enum Error {
    /// 读写模板文件等 IO 错误。
    Io(std::io::Error),
    /// 解码图片失败。
    Image(image::ImageError),
    /// 截图失败(无显示器 / 平台不支持等)。
    Capture(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Image(e) => write!(f, "image error: {e}"),
            Error::Capture(s) => write!(f, "capture error: {s}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Image(e) => Some(e),
            Error::Capture(_) => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<image::ImageError> for Error {
    fn from(e: image::ImageError) -> Self {
        Error::Image(e)
    }
}

/// 便于书写的结果别名。
pub type Result<T> = std::result::Result<T, Error>;
