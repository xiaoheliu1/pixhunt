//! 模板(要去找的那张小图)。

use crate::Result;

/// 一张已解码为 **RGB**(每像素 3 字节)的模板。
#[derive(Clone, Debug)]
pub struct Template {
    pub rgb: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

impl Template {
    /// 从图片文件(PNG 等)加载并转成 RGB。
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let img = image::open(path)?;
        let rgb = img.to_rgb8();
        let (width, height) = (rgb.width() as usize, rgb.height() as usize);
        Ok(Template { rgb: rgb.into_raw(), width, height })
    }

    /// 从已有 RGB 字节构造。
    pub fn from_rgb(rgb: Vec<u8>, width: usize, height: usize) -> Self {
        assert_eq!(rgb.len(), width * height * 3, "template 字节数与尺寸不符");
        Template { rgb, width, height }
    }

    /// 转成单通道灰度(Rec.601 加权),供 ZNCC 等匹配器使用。
    pub fn to_gray(&self) -> Vec<u8> {
        let n = self.width * self.height;
        let mut gray = vec![0u8; n];
        for i in 0..n {
            let b = i * 3;
            let r = self.rgb[b] as u32;
            let g = self.rgb[b + 1] as u32;
            let bl = self.rgb[b + 2] as u32;
            gray[i] = ((r * 299 + g * 587 + bl * 114) / 1000) as u8;
        }
        gray
    }
}
