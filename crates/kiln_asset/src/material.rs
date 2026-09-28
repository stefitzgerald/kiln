use kiln_core::Handle;

use crate::Image;

/// Surface description used by the renderer.
///
/// M0 supports base color (factor × texture × vertex color), optional unlit shading and
/// double-sidedness. Full metallic-roughness PBR arrives in M1.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    /// Optional name (from the source file).
    pub name: Option<String>,
    /// Linear RGBA multiplier.
    pub base_color: [f32; 4],
    /// Optional sRGB base color texture.
    pub base_color_texture: Option<Handle<Image>>,
    /// Skip lighting.
    pub unlit: bool,
    /// Disable back-face culling.
    pub double_sided: bool,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: None,
            base_color: [1.0; 4],
            base_color_texture: None,
            unlit: false,
            double_sided: false,
        }
    }
}

impl Material {
    /// Lit material with a solid linear color.
    pub fn color(rgba: [f32; 4]) -> Self {
        Self {
            base_color: rgba,
            ..Self::default()
        }
    }

    /// Unlit material with a solid linear color.
    pub fn unlit(rgba: [f32; 4]) -> Self {
        Self {
            base_color: rgba,
            unlit: true,
            ..Self::default()
        }
    }

    /// Builder: set the base color texture.
    pub fn with_texture(mut self, texture: Handle<Image>) -> Self {
        self.base_color_texture = Some(texture);
        self
    }
}
