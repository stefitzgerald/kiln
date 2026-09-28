/// How pixel values should be interpreted when sampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColorSpace {
    /// sRGB-encoded color data (base color, emissive). Decoded to linear by the GPU.
    #[default]
    Srgb,
    /// Linear data (normal maps, roughness/metalness, masks).
    Linear,
}

/// 8-bit RGBA image stored on the CPU.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Tightly packed RGBA8 rows, top row first. Length is `width * height * 4`.
    pub data: Vec<u8>,
    /// Color space of the data.
    pub color_space: ColorSpace,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("color_space", &self.color_space)
            .finish_non_exhaustive()
    }
}

impl Image {
    /// Image filled with one color.
    pub fn solid(width: u32, height: u32, rgba: [u8; 4], color_space: ColorSpace) -> Self {
        let data = rgba.repeat((width * height) as usize);
        Self { width, height, data, color_space }
    }

    /// Two-color checkerboard with `cells`×`cells` squares.
    pub fn checkerboard(size: u32, cells: u32, a: [u8; 4], b: [u8; 4]) -> Self {
        let cell = (size / cells.max(1)).max(1);
        let mut data = Vec::with_capacity((size * size * 4) as usize);
        for y in 0..size {
            for x in 0..size {
                data.extend_from_slice(if (x / cell + y / cell) % 2 == 0 { &a } else { &b });
            }
        }
        Self { width: size, height: size, data, color_space: ColorSpace::Srgb }
    }

    /// `true` if the data length matches the dimensions.
    pub fn is_valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.data.len() == self.width as usize * self.height as usize * 4
    }

    /// RGBA of the pixel at `(x, y)`.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [self.data[i], self.data[i + 1], self.data[i + 2], self.data[i + 3]]
    }
}
