//! 模板(要去找的那张小图)。

use crate::Result;

/// 一张已解码为 **RGB**(每像素 3 字节)的模板,可选携带透明掩码。
///
/// `mask` 为 `Some` 时长度等于 `width * height`:
/// `true` = 参与比较,`false` = 跳过(对应原图 alpha==0 的位置)。
/// `None` = 全部像素参与比较(向后兼容)。
#[derive(Clone, Debug)]
pub struct Template {
    pub rgb: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub mask: Option<Vec<bool>>,
}

impl Template {
    /// 从图片文件(PNG / JPEG / WebP)加载并转成 RGB。
    ///
    /// 若图片含 alpha 通道,`alpha == 0` 的像素会被标记为掩码跳过位
    /// (匹配时这些位置不比对)。
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let img = image::open(path)?;
        // 先尝试 RGBA 以检测 alpha
        if img.color().has_alpha() {
            let rgba = img.to_rgba8();
            let (width, height) = (rgba.width() as usize, rgba.height() as usize);
            let mut rgb = Vec::with_capacity(width * height * 3);
            let mut mask = Vec::with_capacity(width * height);
            let mut any_transparent = false;
            for px in rgba.as_raw().as_chunks::<4>().0 {
                rgb.extend_from_slice(&[px[0], px[1], px[2]]);
                let visible = px[3] != 0;
                if !visible {
                    any_transparent = true;
                }
                mask.push(visible);
            }
            Ok(Template {
                rgb,
                width,
                height,
                mask: if any_transparent { Some(mask) } else { None },
            })
        } else {
            let rgb_img = img.to_rgb8();
            let (width, height) = (rgb_img.width() as usize, rgb_img.height() as usize);
            Ok(Template {
                rgb: rgb_img.into_raw(),
                width,
                height,
                mask: None,
            })
        }
    }

    /// 从已有 RGB 字节构造(无掩码,全部像素参与比较)。
    pub fn from_rgb(rgb: Vec<u8>, width: usize, height: usize) -> Self {
        assert_eq!(rgb.len(), width * height * 3, "template 字节数与尺寸不符");
        Template {
            rgb,
            width,
            height,
            mask: None,
        }
    }

    /// 从 RGBA 字节构造,`alpha == 0` 的像素自动变为掩码跳过位。
    ///
    /// `rgba.len()` 必须等于 `width * height * 4`。
    pub fn from_rgba(rgba: Vec<u8>, width: usize, height: usize) -> Self {
        assert_eq!(rgba.len(), width * height * 4, "rgba 字节数与尺寸不符");
        let mut rgb = Vec::with_capacity(width * height * 3);
        let mut mask = Vec::with_capacity(width * height);
        let mut any_transparent = false;
        for px in rgba.as_chunks::<4>().0 {
            rgb.extend_from_slice(&[px[0], px[1], px[2]]);
            let visible = px[3] != 0;
            if !visible {
                any_transparent = true;
            }
            mask.push(visible);
        }
        Template {
            rgb,
            width,
            height,
            mask: if any_transparent { Some(mask) } else { None },
        }
    }

    /// 手动指定掩码:`mask[i] == true` 的像素参与比较,`false` 跳过。
    ///
    /// `mask.len()` 必须等于 `width * height`。
    pub fn with_mask(mut self, mask: Vec<bool>) -> Self {
        assert_eq!(
            mask.len(),
            self.width * self.height,
            "mask 长度与像素数不符"
        );
        self.mask = Some(mask);
        self
    }

    /// 内容指纹(尺寸 + 像素哈希 + 掩码指纹)。用于匹配器缓存预计算结果,内容变则失效。
    pub fn content_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.width.hash(&mut h);
        self.height.hash(&mut h);
        self.rgb.hash(&mut h);
        self.mask.hash(&mut h);
        h.finish()
    }

    /// 转成单通道灰度(Rec.601 加权),供 ZNCC 等匹配器使用。
    ///
    /// 注意:ZNCC 当前**不使用掩码**,所有像素(包括被掩掉的)均参与灰度转换。
    pub fn to_gray(&self) -> Vec<u8> {
        let mut gray = Vec::with_capacity(self.width * self.height);
        for px in self.rgb.as_chunks::<3>().0 {
            let r = px[0] as u32;
            let g = px[1] as u32;
            let b = px[2] as u32;
            gray.push(((r * 299 + g * 587 + b * 114) / 1000) as u8);
        }
        gray
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_rgb_no_mask() {
        let t = Template::from_rgb(vec![128u8; 4 * 4 * 3], 4, 4);
        assert!(t.mask.is_none());
    }

    #[test]
    fn from_rgba_detects_transparent() {
        // 4x4, (1,1) alpha=0, 其余 alpha=255
        let (w, h) = (4usize, 4usize);
        let mut rgba = vec![200u8; w * h * 4];
        rgba[(w + 1) * 4 + 3] = 0; // (1,1) 透明
        let t = Template::from_rgba(rgba, w, h);
        let mask = t.mask.as_ref().expect("应检测到透明像素");
        assert!(!mask[w + 1], "(1,1) 应被标记为跳过");
        assert!(mask[0], "(0,0) 应参与比较");
    }

    #[test]
    fn with_mask_overrides() {
        let t =
            Template::from_rgb(vec![0u8; 2 * 2 * 3], 2, 2).with_mask(vec![true, false, true, true]);
        assert!(!t.mask.as_ref().unwrap()[1]);
    }

    #[test]
    fn content_key_changes_with_mask() {
        let a = Template::from_rgb(vec![10u8; 2 * 2 * 3], 2, 2);
        let b = Template::from_rgb(vec![10u8; 2 * 2 * 3], 2, 2)
            .with_mask(vec![true, false, true, true]);
        assert_ne!(a.content_key(), b.content_key());
    }
}
