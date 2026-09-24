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
        Ok(Template {
            rgb: rgb.into_raw(),
            width,
            height,
        })
    }

    /// 从已有 RGB 字节构造。
    pub fn from_rgb(rgb: Vec<u8>, width: usize, height: usize) -> Self {
        assert_eq!(rgb.len(), width * height * 3, "template 字节数与尺寸不符");
        Template { rgb, width, height }
    }

    /// 内容指纹(尺寸 + 像素哈希)。用于匹配器缓存预计算结果,内容变则失效。
    pub fn content_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.width.hash(&mut h);
        self.height.hash(&mut h);
        self.rgb.hash(&mut h);
        h.finish()
    }

    /// 转成单通道灰度(Rec.601 加权),供 ZNCC 等匹配器使用。
    pub fn to_gray(&self) -> Vec<u8> {
        let mut gray = Vec::with_capacity(self.width * self.height);
        for px in self.rgb.chunks_exact(3) {
            let r = px[0] as u32;
            let g = px[1] as u32;
            let b = px[2] as u32;
            gray.push(((r * 299 + g * 587 + b * 114) / 1000) as u8);
        }
        gray
    }
}
