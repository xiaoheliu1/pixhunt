//! 模板(要去找的那张小图)。

use image::ImageReader;

use crate::Result;

/// 模板像素数上限。**40,000,000** 个像素(约 6324 x 6324)已经远超任何合理的"要去找的
/// 那张小图":真拿这么大一张去 1080p 画面上匹配,一次耗时会长到没有实用价值。
///
/// 设上限是因为 `Template::load` 面对的输入可以是**别人给的图**(下载的素材、用户
/// 拖进来的文件),而"解压炸弹"正是拿几百 KB 的 PNG 在文件头里写巨大尺寸:解码器
/// 按声明的尺寸分配,进程直接被内存打死。32 位目标上还有一层更隐蔽的风险 —— 下面
/// `with_capacity(width * height * 3)` 这类乘法在 release 下不回绕报错,容量算小了
/// 只是后续反复扩容,不会当场失败。
const MAX_TEMPLATE_PIXELS: usize = 40_000_000;

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
    ///
    /// 尺寸超过 4000 万像素(`MAX_TEMPLATE_PIXELS`)时返回 [`crate::Error::Image`]
    /// (变体 `Limits`)而不是继续解码,用于挡住"解压炸弹"式的超大图片。
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self> {
        // 先只读文件头拿尺寸、确认不超上限,才真的解码。直接用 `image::open` 的话,
        // 完整解码(包括那块巨大的输出缓冲)已经跑完了才报错,内存峰值并省不下来。
        // `with_guessed_format` 仍优先按扩展名推断,与原 `image::open` 行为一致。
        // `into_dimensions` 会消耗 reader(它要建解码器才能读 IHDR/SOF),所以尺寸过了
        // 阈值后重新开一次文件 —— 多一次 open 是微秒量级,换的是"不过线就不分配"。
        let path = path.as_ref();
        let (w, h) = ImageReader::open(path)?
            .with_guessed_format()?
            .into_dimensions()?;
        Self::check_template_size(w as usize, h as usize)?;
        let img = image::open(path)?;
        // 先尝试 RGBA 以检测 alpha
        if img.color().has_alpha() {
            let rgba = img.to_rgba8();
            let (width, height) = (rgba.width() as usize, rgba.height() as usize);
            let pixels = Self::check_template_size(width, height)?;
            let mut rgb = Vec::with_capacity(pixels * 3);
            let mut mask = Vec::with_capacity(pixels);
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

    /// 载入前与解码后的尺寸校验:乘法用 `checked_mul`(理由同 [`Template::check_len`]),
    /// 溢出或超过 `MAX_TEMPLATE_PIXELS` 都返回 [`crate::Error::Image`](用 `Limits`
    /// 变体,与 image 自己因尺寸限制拒解时的口径一致)。
    ///
    /// 返回校验过的像素数:后面把它再乘 3 / 4 当容量用时,不需要再防一次回绕。
    ///
    /// 单独拆出来是因为 `load` 需要真图片才能触发这条分支,而测试不想为了验证一个
    /// 上限去生成一张 4000 万像素的 PNG。
    fn check_template_size(width: usize, height: usize) -> Result<usize> {
        let over = || {
            crate::Error::Image(image::ImageError::Limits(
                image::error::LimitError::from_kind(image::error::LimitErrorKind::DimensionError),
            ))
        };
        let pixels = width.checked_mul(height).ok_or_else(over)?;
        if pixels > MAX_TEMPLATE_PIXELS {
            return Err(over());
        }
        Ok(pixels)
    }

    /// 尺寸断言的公共实现:元素个数必须等于 `width * height * bpp`。
    ///
    /// 之所以不直接写 `assert_eq!(len, width * height * bpp)`:release 构建默认关掉
    /// 溢出检查,荒谬尺寸(比如调用方把像素数误当宽度传进来)会让乘法**回绕**成一个
    /// 恰好等于 `len` 的值,断言形同虚设,错误拖到后面按下标取像素才暴露。
    #[inline]
    fn check_len(len: usize, width: usize, height: usize, bpp: usize, what: &str) {
        let want = width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(bpp))
            .unwrap_or_else(|| panic!("{what} 尺寸溢出: {width}x{height}x{bpp}"));
        assert_eq!(len, want, "{what} 元素个数与尺寸不符");
    }

    /// 从已有 RGB 字节构造(无掩码,全部像素参与比较)。
    pub fn from_rgb(rgb: Vec<u8>, width: usize, height: usize) -> Self {
        Self::check_len(rgb.len(), width, height, 3, "template");
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
        Self::check_len(rgba.len(), width, height, 4, "rgba");
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
        Self::check_len(mask.len(), self.width, self.height, 1, "mask");
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
        // 只是容量提示,长度仍由循环决定;用 `saturating_mul` 是为了在荒谬尺寸下
        // 不把乘法回绕成一个小数字(那只会多几次扩容,但没必要留着)。
        let mut gray = Vec::with_capacity(self.width.saturating_mul(self.height));
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

    /// 上限本身的分档测试。不依赖真实图片:为了验证一个阈值去生成一张
    /// 4000 万像素的 PNG 既慢又占地方。
    #[test]
    fn check_template_size_tiers() {
        // 恰好在阈值上:放行
        assert_eq!(
            Template::check_template_size(8000, MAX_TEMPLATE_PIXELS / 8000).unwrap(),
            MAX_TEMPLATE_PIXELS
        );
        // 多一个像素:拒
        assert!(Template::check_template_size(8000, MAX_TEMPLATE_PIXELS / 8000 + 1).is_err());
        // 零尺寸不是错误(尺寸合法性由下游 `Scan::new` 处理,那里直接返回 None)
        assert_eq!(Template::check_template_size(0, 100).unwrap(), 0);
    }

    /// `width * height` 本身就能超出 `usize::MAX`:必须是 `Err` 而不是 panic,
    /// 也不是回绕后的那个小数字。只在 64 位目标上构造得出这种尺寸。
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn check_template_size_rejects_overflow() {
        assert!(Template::check_template_size(1 << 32, 1 << 32).is_err());
    }

    /// 临时文件路径:用进程号 + 测试名避开并发跑时互相踩。
    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("pixhunt_tpl_{name}_{}.png", std::process::id()))
    }

    /// 走一遍真实的"落盘→ `Template::load`"流程,验证换成 `ImageReader` 后
    /// 常规路径没坏(包括扩展名推断格式这条)。
    #[test]
    fn load_roundtrip_reads_a_real_png() {
        let path = temp_path("roundtrip");
        let img = image::DynamicImage::new_rgb8(4, 3);
        img.save(&path).unwrap();
        let t = Template::load(&path).expect("应能载入一张普通 PNG");
        let _ = std::fs::remove_file(&path);
        assert_eq!((t.width, t.height), (4, 3));
        assert_eq!(t.rgb.len(), 4 * 3 * 3);
        assert!(t.mask.is_none());
    }

    /// "解压炸弹":拿一张真 PNG 只改掉文件头里的尺寸声明(后边仍是那 60 多字节)。
    /// `load` 必须在**解码前**就拦住,而不是先按声明尺寸分配完再报错。
    #[test]
    fn load_rejects_oversized_declared_dimensions() {
        let path = temp_path("bomb");
        let img = image::DynamicImage::new_rgb8(4, 3);
        img.save(&path).unwrap();
        // IHDR 块布局(跳过 8 字节 PNG 签名后):[8..12)=数据长度,[12..16)="IHDR",
        // [16..29)=13 字节数据(宽=[16..20) 高=[20..24) 大端,余下色深等),[29..33)=CRC。
        // CRC 盖的是 type+data 即 [12..29),改完尺寸必须重算 —— 不然先撞上的就是
        // 校验和报错,测不到尺寸上限这条分支。
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[16..20].copy_from_slice(&100_000u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&100_000u32.to_be_bytes());
        let crc = crc32(&bytes[12..29]).to_be_bytes();
        bytes[29..33].copy_from_slice(&crc);
        std::fs::write(&path, &bytes).unwrap();

        let err = match Template::load(&path) {
            Ok(_) => panic!("100000x100000 应被上限拦住"),
            Err(e) => e,
        };
        let _ = std::fs::remove_file(&path);
        assert!(
            matches!(err, crate::Error::Image(image::ImageError::Limits(_))),
            "应报成 ImageError::Limits,实际: {err}"
        );
    }

    /// 多项式 0xEDB88320 的 CRC-32(就是 PNG/zlib 用的那个),逐位算即可:
    /// 测试里只跑几十字节,不值得为了它引一个 crc 依赖。
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
}
