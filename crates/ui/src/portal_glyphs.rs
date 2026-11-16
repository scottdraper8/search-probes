use egui::{self, Color32, ColorImage, TextureHandle, TextureOptions, Ui};
use std::io::Cursor;

const GLYPH_SIZE: f32 = 24.0;
const GLYPH_IMAGES: [&[u8]; 16] = [
    include_bytes!("../assets/glyphs/nms-portal/0.png"),
    include_bytes!("../assets/glyphs/nms-portal/1.png"),
    include_bytes!("../assets/glyphs/nms-portal/2.png"),
    include_bytes!("../assets/glyphs/nms-portal/3.png"),
    include_bytes!("../assets/glyphs/nms-portal/4.png"),
    include_bytes!("../assets/glyphs/nms-portal/5.png"),
    include_bytes!("../assets/glyphs/nms-portal/6.png"),
    include_bytes!("../assets/glyphs/nms-portal/7.png"),
    include_bytes!("../assets/glyphs/nms-portal/8.png"),
    include_bytes!("../assets/glyphs/nms-portal/9.png"),
    include_bytes!("../assets/glyphs/nms-portal/a.png"),
    include_bytes!("../assets/glyphs/nms-portal/b.png"),
    include_bytes!("../assets/glyphs/nms-portal/c.png"),
    include_bytes!("../assets/glyphs/nms-portal/d.png"),
    include_bytes!("../assets/glyphs/nms-portal/e.png"),
    include_bytes!("../assets/glyphs/nms-portal/f.png"),
];

pub struct PortalGlyphs {
    textures: [TextureHandle; 16],
}

impl PortalGlyphs {
    pub fn new(context: &egui::Context) -> Self {
        let textures: Vec<_> = GLYPH_IMAGES
            .iter()
            .enumerate()
            .map(|(index, bytes)| {
                context.load_texture(
                    format!("portal-glyph-{index:X}"),
                    decode(bytes).expect("bundled portal glyph must be a valid RGBA PNG"),
                    TextureOptions::LINEAR,
                )
            })
            .collect();
        Self {
            textures: textures
                .try_into()
                .unwrap_or_else(|_| unreachable!("the complete glyph atlas has 16 textures")),
        }
    }

    pub fn show(&self, ui: &mut Ui, portal_code: &str, tint: Color32) -> egui::Response {
        let indices =
            indices(portal_code).expect("generated portal code must contain 12 hex glyphs");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (glyph, index) in portal_code.chars().zip(indices) {
                ui.add(
                    egui::Image::from_texture((
                        self.textures[usize::from(index)].id(),
                        egui::Vec2::splat(GLYPH_SIZE),
                    ))
                    .tint(tint)
                    .alt_text(format!("Portal glyph {glyph}")),
                );
            }
        })
        .response
    }
}

fn decode(bytes: &[u8]) -> Result<ColorImage, String> {
    let mut reader = png::Decoder::new(Cursor::new(bytes))
        .read_info()
        .map_err(|error| error.to_string())?;
    let mut pixels = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or("portal glyph dimensions exceed the platform address space")?
    ];
    let info = reader
        .next_frame(&mut pixels)
        .map_err(|error| error.to_string())?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err("portal glyph must use 8-bit RGBA pixels".to_owned());
    }
    let mut normalized = Vec::with_capacity(info.buffer_size());
    for pixel in pixels[..info.buffer_size()].as_chunks::<4>().0 {
        normalized.extend_from_slice(&[255, 255, 255, pixel[3]]);
    }
    Ok(ColorImage::from_rgba_unmultiplied(
        [info.width as usize, info.height as usize],
        &normalized,
    ))
}

fn indices(portal_code: &str) -> Result<[u8; 12], &'static str> {
    if portal_code.len() != 12 {
        return Err("portal code must contain 12 glyphs");
    }
    let mut indices = [0; 12];
    for (index, value) in portal_code.bytes().enumerate() {
        indices[index] = match value {
            b'0'..=b'9' => value - b'0',
            b'A'..=b'F' => value - b'A' + 10,
            _ => return Err("portal code must use uppercase hexadecimal glyphs"),
        };
    }
    Ok(indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_code_maps_directly_to_the_sixteen_glyph_textures() {
        assert_eq!(
            indices("1001817FF7FF"),
            Ok([1, 0, 0, 1, 8, 1, 7, 15, 15, 7, 15, 15])
        );
        assert!(indices("1001817ff7ff").is_err());
        assert!(indices("1001817FF7F").is_err());
    }

    #[test]
    fn bundled_glyphs_decode_to_matching_square_alpha_masks() {
        for bytes in GLYPH_IMAGES {
            let image = decode(bytes).unwrap();
            assert_eq!(image.size, [256, 256]);
            assert!(image.pixels.iter().any(|pixel| pixel.a() == 0));
            assert!(image.pixels.iter().any(|pixel| pixel.a() == 255));
            assert!(image.pixels.iter().all(|pixel| {
                pixel.a() == 0 || pixel.to_srgba_unmultiplied()[..3] == [255, 255, 255]
            }));
        }
    }
}
