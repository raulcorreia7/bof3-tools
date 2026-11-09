//! Draws a field scene as the PlayStation GPU draws its primitives, into an RGBA frame that any window toolkit can show. A [`Camera`] projects the corners the way the GTE does (`screen = centre + focal * (x, y) / z`), and a renderer fills the frame: corners 0, 1, 2 and 1, 2, 3, texture coordinates are affine in screen space, a texel is read through its CLUT, the colour `0x0000` is transparent, texels are multiplied by the vertex colour with `0x80` as one, and a semi-transparent primitive mixes the texels whose bit 15 is set by one of the four modes (psx-spx, "GPU Rendering Attributes" and "GPU Texpage Attribute").

#![deny(unsafe_op_in_unsafe_fn)]

pub mod camera;
#[cfg(feature = "gles")]
pub mod gles;
pub mod math;
pub mod software;

pub use camera::Camera;

/// VRAM width in 16-bit words (psx-spx, "GPU Video Memory (VRAM)").
pub const VRAM_WIDTH: usize = 1024;
/// VRAM height in lines.
pub const VRAM_HEIGHT: usize = 512;
/// The vertex colour that leaves a texel unchanged (psx-spx, "GPU Rendering Attributes").
pub const NEUTRAL_SHADE: u8 = 0x80;
/// The semi-transparency bit of a colour word.
pub const STP_BIT: u16 = 0x8000;

/// How a primitive's pixels mix with the frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Blend {
    /// Every drawn pixel replaces the frame.
    Opaque,
    /// The GPU's semi-transparency mode `0` to `3`; texels without bit 15 are opaque.
    Psx(u8),
    /// The colour's alpha mixes it over the frame; an editor overlay, not a GPU mode.
    Alpha,
}

/// A texel depth of a VRAM texture page.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Depth {
    Four,
    Eight,
    Fifteen,
}

impl Depth {
    /// Texels per VRAM word.
    pub fn per_word(self) -> usize {
        match self {
            Self::Four => 4,
            Self::Eight => 2,
            Self::Fifteen => 1,
        }
    }
}

/// Where a primitive's texels come from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Texture {
    /// Untextured: the colour fills the polygon.
    None,
    /// A VRAM texture page read through a CLUT, at byte coordinates per corner.
    Vram { page: [u16; 2], clut: [u16; 2], depth: Depth, uvs: [[u8; 2]; 4] },
    /// One of the scene's images, at fractional coordinates per corner (`0` to `1`).
    Image { image: usize, uvs: [[f32; 2]; 4] },
}

/// One four-corner polygon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Primitive {
    /// Model-space corners; the GPU draws the triangles 0, 1, 2 and 1, 2, 3.
    pub corners: [[f32; 3]; 4],
    pub texture: Texture,
    /// The vertex colour: for a texture, the multiplier (`0x80` is one) and, for an image, the alpha; for no texture, the fill.
    pub colour: [u8; 4],
    pub blend: Blend,
    /// Whether the primitive hides behind nearer ones; an overlay drawn on top clears it.
    pub depth_test: bool,
    /// What [`pick`] returns for the primitive; `None` for one that cannot be picked.
    pub id: Option<u32>,
}

/// An RGBA image a primitive can show.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    /// Row-major RGBA bytes.
    pub rgba: Vec<u8>,
}

/// Everything a frame is drawn from.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    /// `VRAM_WIDTH * VRAM_HEIGHT` colour words, row-major; empty for a scene with no VRAM texture.
    pub vram: Vec<u16>,
    pub images: Vec<Image>,
    pub primitives: Vec<Primitive>,
    /// The frame's clear colour.
    pub background: [u8; 4],
}

/// A drawn frame.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    /// Row-major RGBA bytes, top row first.
    pub rgba: Vec<u8>,
}

impl Frame {
    /// The pixel at `x`, `y`.
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let at = (y * self.width + x) * 4;
        [self.rgba[at], self.rgba[at + 1], self.rgba[at + 2], self.rgba[at + 3]]
    }
}

/// Which renderer draws.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Backend {
    Software,
    Gles,
}

/// A renderer: OpenGL ES when the feature is on and a context opens, else the software path.
pub struct Renderer {
    #[cfg(feature = "gles")]
    gles: Option<gles::Gles>,
    /// Why the OpenGL ES path is not in use, when it is not.
    pub fallback: Option<String>,
}

impl Renderer {
    /// Opens the preferred backend.
    pub fn new(prefer_gles: bool) -> Self {
        #[cfg(feature = "gles")]
        {
            if prefer_gles {
                return match gles::Gles::new() {
                    Ok(gles) => Self { gles: Some(gles), fallback: None },
                    Err(error) => Self { gles: None, fallback: Some(error) },
                };
            }
            Self { gles: None, fallback: Some("the software renderer was chosen".into()) }
        }
        #[cfg(not(feature = "gles"))]
        {
            let _ = prefer_gles;
            Self { fallback: Some("built without the gles feature".into()) }
        }
    }

    /// The backend that draws.
    pub fn backend(&self) -> Backend {
        #[cfg(feature = "gles")]
        if self.gles.is_some() {
            return Backend::Gles;
        }
        Backend::Software
    }

    /// Draws `scene` through `camera` into a `width` x `height` frame.
    pub fn draw(&mut self, scene: &Scene, camera: &Camera, width: usize, height: usize) -> Frame {
        #[cfg(feature = "gles")]
        if let Some(gles) = self.gles.as_mut() {
            match gles.draw(scene, camera, width, height) {
                Ok(frame) => return frame,
                Err(error) => {
                    self.gles = None;
                    self.fallback = Some(error);
                }
            }
        }
        software::draw(scene, camera, width, height)
    }
}

/// The primitives that draw last: semi-transparent and overlay ones, far to near; then the overlays that ignore depth.
pub fn draw_order(scene: &Scene, camera: &Camera) -> Vec<usize> {
    let mut opaque = Vec::new();
    let mut mixed = Vec::new();
    let mut top = Vec::new();
    for (index, primitive) in scene.primitives.iter().enumerate() {
        if !primitive.depth_test {
            top.push(index);
        } else if primitive.blend == Blend::Opaque {
            opaque.push(index);
        } else {
            let depth = primitive.corners.iter().map(|&corner| camera.view(corner)[2]).fold(f32::MIN, f32::max);
            mixed.push((depth, index));
        }
    }
    mixed.sort_by(|left, right| right.0.total_cmp(&left.0));
    opaque.extend(mixed.into_iter().map(|(_, index)| index));
    opaque.extend(top);
    opaque
}

/// The pickable primitive nearest the camera under frame pixel `x`, `y` of a `width` x `height` frame: its id and its view depth.
pub fn pick(scene: &Scene, camera: &Camera, width: usize, height: usize, x: f32, y: f32) -> Option<u32> {
    let mut best: Option<(bool, f32, u32)> = None;
    for primitive in &scene.primitives {
        let Some(id) = primitive.id else { continue };
        let view = primitive.corners.map(|corner| camera.view(corner));
        if view.iter().any(|corner| corner[2] <= camera.near) {
            continue;
        }
        let screen = view.map(|corner| camera.project(corner, width, height));
        for [a, b, c] in [[0, 1, 2], [1, 2, 3]] {
            let Some([wa, wb, wc]) = math::barycentric(screen[a], screen[b], screen[c], [x, y]) else { continue };
            let inverse = wa / view[a][2] + wb / view[b][2] + wc / view[c][2];
            let depth = 1.0 / inverse;
            let on_top = !primitive.depth_test;
            let better = match best {
                None => true,
                Some((best_top, best_depth, _)) => (on_top && !best_top) || (on_top == best_top && depth < best_depth),
            };
            if better {
                best = Some((on_top, depth, id));
            }
        }
    }
    best.map(|(_, _, id)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(depth: f32, id: u32, colour: [u8; 4]) -> Primitive {
        Primitive {
            corners: [[-1.0, -1.0, depth], [1.0, -1.0, depth], [-1.0, 1.0, depth], [1.0, 1.0, depth]],
            texture: Texture::None,
            colour,
            blend: Blend::Opaque,
            depth_test: true,
            id: Some(id),
        }
    }

    #[test]
    fn the_nearest_primitive_is_picked() {
        let camera = Camera::looking_down_z(100.0);
        let scene = Scene { primitives: vec![square(10.0, 1, [255; 4]), square(5.0, 2, [255; 4])], ..Scene::default() };
        assert_eq!(pick(&scene, &camera, 64, 64, 32.0, 32.0), Some(2));
        assert_eq!(pick(&scene, &camera, 64, 64, 0.0, 0.0), None, "outside both squares");
    }
}
