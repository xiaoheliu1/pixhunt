//! 一帧画面的统一表示(截图后端产出它,匹配器消费它)。

/// 每像素 4 字节时的通道顺序。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PixelFormat {
    Rgba8,
    Bgra8,
}

/// 帧内的一个矩形区域(像素坐标,左上原点)。用于限定查找范围。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Rect {
    pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Rect {
            x,
            y,
            width,
            height,
        }
    }
}

/// 允许用 `(x, y, width, height)` 元组直接当区域传入。
impl From<(usize, usize, usize, usize)> for Rect {
    fn from(v: (usize, usize, usize, usize)) -> Self {
        Rect::new(v.0, v.1, v.2, v.3)
    }
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
    pub fn rgba8(width: usize, height: usize, pixels: Vec<u8>) -> Self {
        Frame {
            width,
            height,
            pixels,
            format: PixelFormat::Rgba8,
        }
    }

    pub fn bgra8(width: usize, height: usize, pixels: Vec<u8>) -> Self {
        Frame {
            width,
            height,
            pixels,
            format: PixelFormat::Bgra8,
        }
    }

    /// 就地把本帧设为 w x h 的 BGRA,并保证 `pixels` 有 `w*h*4` 长度。
    /// 复用已有容量:热路径(逐帧抓屏)下不会每帧重新分配。
    pub fn prepare_bgra(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.format = PixelFormat::Bgra8;
        self.pixels.clear();
        self.pixels.resize(width * height * 4, 0);
    }

    /// 同 [`Frame::prepare_bgra`],但通道顺序为 RGBA。
    pub fn prepare_rgba(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.format = PixelFormat::Rgba8;
        self.pixels.clear();
        self.pixels.resize(width * height * 4, 0);
    }

    /// 整帧大小的区域。
    pub fn full_rect(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

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

    /// 把任意区域裁剪到帧边界内(保证 x+width<=w, y+height<=h)。
    pub fn clamp(&self, r: Rect) -> Rect {
        let x = r.x.min(self.width);
        let y = r.y.min(self.height);
        let width = r.width.min(self.width - x);
        let height = r.height.min(self.height - y);
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// 裁剪出一个子帧(拷贝像素,保持通道格式)。
    pub fn crop(&self, r: Rect) -> Frame {
        let r = self.clamp(r);
        let row = r.width * 4;
        let mut out = Vec::with_capacity(r.width * r.height * 4);
        for y in 0..r.height {
            let src = ((r.y + y) * self.width + r.x) * 4;
            out.extend_from_slice(&self.pixels[src..src + row]);
        }
        Frame {
            pixels: out,
            width: r.width,
            height: r.height,
            format: self.format,
        }
    }

    /// 转成单通道灰度(Rec.601 加权),长度 = width * height。
    pub fn to_gray(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(self.width * self.height);
        self.to_gray_into(&mut v);
        v
    }

    /// 就地把帧转灰度写入 `out`(复用其容量),结束时 `out.len() == width*height`。
    pub fn to_gray_into(&self, out: &mut Vec<u8>) {
        let (ro, go, bo) = self.rgb_offsets();
        out.clear();
        out.reserve(self.width * self.height);
        for px in self.pixels.chunks_exact(4) {
            let r = px[ro] as u32;
            let g = px[go] as u32;
            let bl = px[bo] as u32;
            out.push(((r * 299 + g * 587 + bl * 114) / 1000) as u8);
        }
    }
}
