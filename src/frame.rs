//! 一帧画面的统一表示(截图后端产出它,匹配器消费它)。

/// 每像素 4 字节时的通道顺序。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PixelFormat {
    Rgba8,
    Bgra8,
}

/// 一张图像:连续像素 + 尺寸 + 通道顺序。
///
/// 约定:每像素 4 字节、行无填充(stride = width * 4);模板则为每像素 3 字节、
/// 固定 **RGB** 顺序([`crate::Template`])。匹配器依据 [`Frame::rgb_offsets`]
/// 做通道映射,因此 screenshots(RGBA)与 GDI / DXGI(BGRA)都能零转换直接匹配。
#[derive(Clone, Debug)]
pub struct Frame {
    pub pixels: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub format: PixelFormat,
}

impl Frame {
    /// 以 RGBA8 构造一帧。
    pub fn rgba8(width: usize, height: usize, pixels: Vec<u8>) -> Self {
        Frame { width, height, pixels, format: PixelFormat::Rgba8 }
    }

    /// 以 BGRA8 构造一帧(GDI / DXGI 后端产出)。
    pub fn bgra8(width: usize, height: usize, pixels: Vec<u8>) -> Self {
        Frame { width, height, pixels, format: PixelFormat::Bgra8 }
    }

    /// 每行字节数。
    #[inline]
    pub fn stride(&self) -> usize {
        self.width * 4
    }

    /// R / G / B 在一个 4 字节像素内的字节偏移。
    #[inline]
    pub fn rgb_offsets(&self) -> (usize, usize, usize) {
        match self.format {
            PixelFormat::Rgba8 => (0, 1, 2),
            PixelFormat::Bgra8 => (2, 1, 0),
        }
    }

    /// 转成单通道灰度(Rec.601 加权),长度 = width * height。
    pub fn to_gray(&self) -> Vec<u8> {
        let (ro, go, bo) = self.rgb_offsets();
        let n = self.width * self.height;
        let mut gray = vec![0u8; n];
        for i in 0..n {
            let b = i * 4;
            let r = self.pixels[b + ro] as u32;
            let g = self.pixels[b + go] as u32;
            let bl = self.pixels[b + bo] as u32;
            gray[i] = ((r * 299 + g * 587 + bl * 114) / 1000) as u8;
        }
        gray
    }
}
