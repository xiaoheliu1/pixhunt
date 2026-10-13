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
/// 做通道映射,因此 xcap(RGBA)与 GDI / DXGI(BGRA)都能零转换直接匹配。
#[derive(Clone, Debug)]
pub struct Frame {
    pub pixels: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub format: PixelFormat,
}

impl Frame {
    /// 用 RGBA 字节建帧。`pixels.len()` **必须**等于 `width * height * 4`,
    /// 不一致时当场 `panic`(与 [`crate::Template::from_rgb`] 同一口径:公开构造
    /// 函数的输入契约违反就硬失败,而不是留到背面的匹配路径里读越界。
    pub fn rgba8(width: usize, height: usize, pixels: Vec<u8>) -> Self {
        check_pixel_len(pixels.len(), width, height, "Frame::rgba8");
        Frame {
            width,
            height,
            pixels,
            format: PixelFormat::Rgba8,
        }
    }

    /// 用 BGRA 字节建帧。长度校验与 [`Frame::rgba8`] 完全相同(也是同样会当场
    /// `panic` 的契约违反)。
    pub fn bgra8(width: usize, height: usize, pixels: Vec<u8>) -> Self {
        check_pixel_len(pixels.len(), width, height, "Frame::bgra8");
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
        self.prepare(width, height, PixelFormat::Bgra8);
    }

    /// 同 [`Frame::prepare_bgra`],但通道顺序为 RGBA。
    pub fn prepare_rgba(&mut self, width: usize, height: usize) {
        self.prepare(width, height, PixelFormat::Rgba8);
    }

    fn prepare(&mut self, width: usize, height: usize, format: PixelFormat) {
        self.width = width;
        self.height = height;
        self.format = format;
        let want = width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(4))
            .unwrap_or_else(|| panic!("Frame::prepare: {width}x{height} 尺寸溢出"));
        // 长度已经是 `want` 时**一个字节都不碰**:原来的 `clear()` + `resize()` 会把
        // 整帧抹零一遍(1920x1200 实测 2.3 µs/帧),而抓取方紧接着就要全量覆写它,
        // 这次填充没有语义价值。尺寸真的变了才重铺(新缓冲留 0,不残留上一帧内容)。
        if self.pixels.len() != want {
            self.pixels.clear();
            self.pixels.resize(want, 0);
        }
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
    ///
    /// 顺便在调试构建里钉住"缓冲长度 = width*height*4"这个不变量:每个消费方
    /// (匹配、逐帧比较、转灰度)都要算一次行下标,后端交出一个尺寸与缓冲不符的帧时,
    /// 在这里报错远比在后面读到错误数据、或踩到切片越界 panic 好查。
    #[inline]
    pub fn rgb_offsets(&self) -> (usize, usize, usize) {
        debug_assert_eq!(
            self.pixels.len(),
            self.width.saturating_mul(self.height).saturating_mul(4),
            "帧的像素缓冲与尺寸不符: len={} 而 {}x{} 应为 {}",
            self.pixels.len(),
            self.width,
            self.height,
            self.width.saturating_mul(self.height).saturating_mul(4)
        );
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
        for px in self.pixels.as_chunks::<4>().0 {
            let r = px[ro] as u32;
            let g = px[go] as u32;
            let bl = px[bo] as u32;
            out.push(((r * 299 + g * 587 + bl * 114) / 1000) as u8);
        }
    }
}

/// 校验像素长度与尺寸一致。乘法走 `checked_mul`:`width * height * 4` 在 release
/// 下回绕会算出一个**变小**的期望值,恰好让普通的 `len == w*h*4` 判断通过。
///
/// 这里选 `panic` 而不是返回 `Result`,与 [`crate::Template::from_rgb`] 保持一致:
/// 它们是公开构造函数,调用方传错就是代码写错了,当场报比让后面的匹配路径
/// 读到缓冲区之外(或 `as_chunks` 静默少算尾部)好查得多。
fn check_pixel_len(len: usize, width: usize, height: usize, what: &str) {
    let want = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(4))
        .unwrap_or_else(|| panic!("{what}: {width}x{height} 尺寸溢出"));
    assert_eq!(
        len, want,
        "{what}: pixels 长度({len}) != width*height*4({want})"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 公开构造函数的契约违反必须当场 `panic`(而不是拖到匹配路径里读脏数据)。
    #[test]
    #[should_panic(expected = "pixels 长度")]
    fn rgba8_rejects_short_buffer() {
        let _ = Frame::rgba8(4, 4, vec![0u8; 63]);
    }

    /// 乘法回绕也算不出来:release 默认不查溢出,`width * height * 4` 可以回绕成
    /// 恰好等于 `len` 的值。这里构造一个相乘会超过 `usize::MAX` 的形状,
    /// `checked_mul` 必须当场报溢出而不是放行。
    #[test]
    #[cfg(target_pointer_width = "64")]
    #[should_panic(expected = "尺寸溢出")]
    fn rgba8_rejects_overflowing_shape() {
        // (1<<32) * (1<<32) * 4 远超 usize::MAX
        let _ = Frame::rgba8(1 << 32, 1 << 32, Vec::new());
    }

    /// 尺寸与缓冲不符的帧(只能从后端内部写坏,公开构造函数拦不住)在调试
    /// 构建里必须被 `rgb_offsets` 当场提示。release 下 `debug_assert` 被编译掉,不测。
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "帧的像素缓冲与尺寸不符")]
    fn rgb_offsets_notices_shape_mismatch() {
        let f = Frame {
            pixels: vec![0u8; 8],
            width: 4,
            height: 4,
            format: PixelFormat::Rgba8,
        };
        f.rgb_offsets();
    }

    /// `prepare_*` 在尺寸不变时不得改写像素(热路径靠这点省掉整帧 memset)。
    #[test]
    fn prepare_keeps_content_when_size_unchanged() {
        let mut f = Frame::bgra8(2, 2, vec![7u8; 2 * 2 * 4]);
        f.prepare_bgra(2, 2);
        assert!(f.pixels.iter().all(|&v| v == 7));
        // 尺寸变了才重铺,且不残留上一帧内容
        f.prepare_bgra(1, 1);
        assert_eq!(f.pixels, vec![0u8; 4]);
        assert_eq!((f.width, f.height, f.stride()), (1, 1, 4));
    }

    #[test]
    fn clamp_and_crop_stay_inside_frame() {
        let f = Frame::rgba8(10, 8, vec![3u8; 10 * 8 * 4]);
        // 起点就在帧外:宽/高夹成 0,不 panic
        assert_eq!(f.clamp(Rect::new(12, 30, 5, 5)), Rect::new(10, 8, 0, 0));
        let c = f.crop(Rect::new(8, 6, 100, 100));
        assert_eq!((c.width, c.height), (2, 2));
        assert_eq!(c.pixels.len(), 2 * 2 * 4);
        // 夹到边界后的 crop 与直接给准确尺寸等价
        let d = f.crop(Rect::new(8, 6, 2, 2));
        assert_eq!((d.width, d.height, &d.pixels[..]), (2, 2, &c.pixels[..]));
        assert_eq!(d.format, c.format);
    }
}
