//! 统一的错误类型与 `Result` 别名。
use std::fmt;

/// pixhunt 的错误。
#[derive(Debug)]
pub enum Error {
    /// 读写模板文件等 IO 错误。
    Io(std::io::Error),
    /// 解码图片失败。
    Image(image::ImageError),
    /// 截图失败(无显示器 / 平台不支持 / 后端权限被拒等)。
    ///
    /// `message` 是**在哪一步失败的上下文**,`source` 保留底层错误对象。
    /// 之前只存一个 `String`:像 xcap 那样大量用 `#[error(transparent)]` 的库,
    /// 一层 `Display` 确实还在,但它自己包的 io / D-Bus / Win32 错误就再也取不出来了
    /// —— 排查"Linux 上为何黑屏"靠的正是那几层。
    Capture {
        message: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

impl Error {
    /// 后端自行判定出的失败(没有底层错误对象可带)。
    pub fn capture(message: impl Into<String>) -> Self {
        Error::Capture {
            message: message.into(),
            source: None,
        }
    }

    /// 包装一个底层错误:`message` 写明失败发生在哪步,原始错误挂在
    /// [`source`](std::error::Error::source) 上供逐层遍历。
    pub fn capture_from(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Error::Capture {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Image(e) => write!(f, "image error: {e}"),
            Error::Capture { message, source } => match source {
                // 把底层原因拼进 Display,保证只打印一次错误也不会失去信息。
                Some(src) => write!(f, "capture error: {message}: {src}"),
                None => write!(f, "capture error: {message}"),
            },
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Image(e) => Some(e),
            Error::Capture { source, .. } => source
                .as_ref()
                .map(|src| &**src as &(dyn std::error::Error + 'static)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    /// 沿 `source` 往回找最底层的 io 错误。
    fn root_io_kind(err: &Error) -> Option<std::io::ErrorKind> {
        let mut cur = err.source();
        while let Some(e) = cur {
            if let Some(io) = e.downcast_ref::<std::io::Error>() {
                return Some(io.kind());
            }
            cur = e.source();
        }
        None
    }

    #[test]
    fn capture_without_source_reads_as_plain_message() {
        let err = Error::capture("no display found");
        assert_eq!(err.to_string(), "capture error: no display found");
        assert!(err.source().is_none());
    }

    #[test]
    fn capture_from_keeps_the_source_chain() {
        // 模拟底层库的做法:真正的 io 错误被包在它自己的错误类型里。
        let inner = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "pipewire denied");
        let err = Error::capture_from("xcap Monitor::capture_image()", inner);
        // 只打印一次错误也得看见原因……
        let text = err.to_string();
        assert!(text.contains("Monitor::capture_image"), "{text}");
        assert!(text.contains("pipewire denied"), "{text}");
        // ……同时还能按类型取回底层错误,这才是保留链的意义。
        assert_eq!(
            root_io_kind(&err),
            Some(std::io::ErrorKind::PermissionDenied)
        );
    }
}
