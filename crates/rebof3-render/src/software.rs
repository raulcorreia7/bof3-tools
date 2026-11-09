//! The software renderer: the GPU's polygon rules on the CPU, with a depth buffer.

use crate::math::{self, barycentric};
use crate::{Blend, Camera, Depth, Frame, Image, NEUTRAL_SHADE, Primitive, STP_BIT, Scene, Texture, VRAM_HEIGHT, VRAM_WIDTH, draw_order};

/// A clipped corner: its view position and texture coordinate.
#[derive(Clone, Copy)]
struct Corner {
    view: [f32; 3],
    uv: [f32; 2],
}

/// The two triangles the GPU splits a four-corner polygon into (psx-spx, "GPU Render Polygon
/// Commands": a quad is drawn as the triangles 1-2-3 and 2-3-4).
pub const TRIANGLES: [[usize; 3]; 2] = [[0, 1, 2], [1, 2, 3]];

/// Draws `scene` through `camera` into a `width` x `height` frame.
pub fn draw(scene: &Scene, camera: &Camera, width: usize, height: usize) -> Frame {
    let mut target = Target { width, height, rgb: vec![[scene.background[0], scene.background[1], scene.background[2]]; width * height], depth: vec![0.0; width * height] };
    for index in draw_order(scene, camera) {
        target.primitive(scene, camera, &scene.primitives[index]);
    }
    // The frame is opaque, as the GPU's display is.
    let rgba = target.rgb.iter().flat_map(|&[red, green, blue]| [red, green, blue, 0xFF]).collect();
    Frame { width, height, rgba }
}

/// The frame being drawn: colours and the inverse view depth of the nearest opaque pixel (0 is
/// empty).
struct Target {
    width: usize,
    height: usize,
    rgb: Vec<[u8; 3]>,
    depth: Vec<f32>,
}

impl Target {
    fn primitive(&mut self, scene: &Scene, camera: &Camera, primitive: &Primitive) {
        let uvs: [[f32; 2]; 4] = match primitive.texture {
            Texture::None => [[0.0; 2]; 4],
            Texture::Vram { uvs, .. } => uvs.map(|[u, v]| [f32::from(u), f32::from(v)]),
            Texture::Image { uvs, .. } => uvs,
        };
        let corners: [Corner; 4] = std::array::from_fn(|corner| Corner { view: camera.view(primitive.corners[corner]), uv: uvs[corner] });
        for triangle in TRIANGLES {
            let clipped = clip(triangle.map(|corner| corners[corner]), camera.near);
            for fan in 1..clipped.len().saturating_sub(1) {
                self.triangle(scene, camera, primitive, [clipped[0], clipped[fan], clipped[fan + 1]]);
            }
        }
    }

    fn triangle(&mut self, scene: &Scene, camera: &Camera, primitive: &Primitive, corners: [Corner; 3]) {
        let screen = corners.map(|corner| camera.project(corner.view, self.width, self.height));
        let left = screen.iter().map(|point| point[0]).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
        let top = screen.iter().map(|point| point[1]).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
        let right = screen.iter().map(|point| point[0]).fold(f32::MIN, f32::max).ceil().min(self.width as f32) as usize;
        let bottom = screen.iter().map(|point| point[1]).fold(f32::MIN, f32::max).ceil().min(self.height as f32) as usize;
        for y in top..bottom {
            for x in left..right {
                let centre = [x as f32 + 0.5, y as f32 + 0.5];
                let Some(weights) = barycentric(screen[0], screen[1], screen[2], centre) else { continue };
                let inverse: f32 = (0..3).map(|corner| weights[corner] / corners[corner].view[2]).sum();
                let at = y * self.width + x;
                if primitive.depth_test && inverse <= self.depth[at] {
                    continue;
                }
                let uv: [f32; 2] = [0, 1].map(|axis| (0..3).map(|corner| weights[corner] * corners[corner].uv[axis]).sum());
                let Some(fragment) = shade(scene, primitive, uv) else { continue };
                self.write(at, primitive, fragment, inverse);
            }
        }
    }

    /// Writes one fragment by the primitive's blend.
    fn write(&mut self, at: usize, primitive: &Primitive, fragment: Fragment, inverse: f32) {
        let behind = self.rgb[at];
        let (colour, solid) = match (primitive.blend, fragment.mix) {
            (Blend::Opaque, _) | (Blend::Psx(_), false) => (fragment.rgb, true),
            (Blend::Psx(mode), true) => (std::array::from_fn(|channel| psx_mix(mode, behind[channel], fragment.rgb[channel])), false),
            (Blend::Alpha, _) => {
                let alpha = u32::from(fragment.alpha);
                let mix = |b: u8, f: u8| ((u32::from(f) * alpha + u32::from(b) * (255 - alpha)) / 255) as u8;
                (std::array::from_fn(|channel| mix(behind[channel], fragment.rgb[channel])), fragment.alpha == 0xFF)
            }
        };
        self.rgb[at] = colour;
        if solid && primitive.depth_test {
            self.depth[at] = inverse;
        }
    }
}

/// A shaded texel: its colour, whether it mixes with the frame, and its alpha for [`Blend::Alpha`].
struct Fragment {
    rgb: [u8; 3],
    mix: bool,
    alpha: u8,
}

/// The fragment of `primitive` at texture coordinate `uv`, or `None` where the texel is transparent.
fn shade(scene: &Scene, primitive: &Primitive, uv: [f32; 2]) -> Option<Fragment> {
    let [red, green, blue, alpha] = primitive.colour;
    match primitive.texture {
        Texture::None => Some(Fragment { rgb: [red, green, blue], mix: true, alpha }),
        Texture::Vram { page, clut, depth, .. } => {
            let word = fetch(&scene.vram, page, clut, depth, uv[0] as u8, uv[1] as u8);
            if word == 0 {
                return None;
            }
            let texel = word_rgb(word);
            let shade = [red, green, blue];
            let rgb = std::array::from_fn(|channel| modulate(texel[channel], shade[channel]));
            Some(Fragment { rgb, mix: word & STP_BIT != 0, alpha: 0xFF })
        }
        Texture::Image { image, .. } => {
            let image = scene.images.get(image)?;
            let texel = sample(image, uv)?;
            let alpha = ((u32::from(texel[3]) * u32::from(alpha)) / 255) as u8;
            (alpha != 0).then_some(Fragment { rgb: [texel[0], texel[1], texel[2]], mix: true, alpha })
        }
    }
}

/// The colour word the GPU reads at texel `u`, `v` of a page through a CLUT (psx-spx, "GPU Texture
/// Caching": a 4-bit texel is nibble `u % 4` of word `u / 4`, an 8-bit one byte `u % 2` of word
/// `u / 2`, low first). An empty VRAM reads as zero.
pub fn fetch(vram: &[u16], page: [u16; 2], clut: [u16; 2], depth: Depth, u: u8, v: u8) -> u16 {
    if vram.len() < VRAM_WIDTH * VRAM_HEIGHT {
        return 0;
    }
    let per_word = depth.per_word();
    let x = (usize::from(page[0]) + usize::from(u) / per_word) % VRAM_WIDTH;
    let y = (usize::from(page[1]) + usize::from(v)) % VRAM_HEIGHT;
    let word = vram[y * VRAM_WIDTH + x];
    if depth == Depth::Fifteen {
        return word;
    }
    let bits = 16 / per_word;
    let index = (usize::from(word) >> ((usize::from(u) % per_word) * bits)) & ((1 << bits) - 1);
    let column = (usize::from(clut[0]) + index) % VRAM_WIDTH;
    vram[usize::from(clut[1]) % VRAM_HEIGHT * VRAM_WIDTH + column]
}

/// The 8-bit channels of a colour word: each 5-bit channel shifted up by three (psx-spx, "GPU Color
/// Depth").
pub fn word_rgb(word: u16) -> [u8; 3] {
    [0, 5, 10].map(|shift| (((word >> shift) & 0x1F) as u8) << 3)
}

/// A texel channel times a vertex colour channel, where [`NEUTRAL_SHADE`] is one.
pub fn modulate(texel: u8, shade: u8) -> u8 {
    (u32::from(texel) * u32::from(shade) / u32::from(NEUTRAL_SHADE)).min(255) as u8
}

/// The GPU's semi-transparency of `front` over `back` in mode `0` to `3`.
pub fn psx_mix(mode: u8, back: u8, front: u8) -> u8 {
    let (back, front) = (i32::from(back), i32::from(front));
    let value = match mode & 3 {
        0 => (back + front) / 2,
        1 => back + front,
        2 => back - front,
        _ => back + front / 4,
    };
    value.clamp(0, 255) as u8
}

/// The nearest texel of an image at fractional `uv`, or `None` past its edge.
fn sample(image: &Image, uv: [f32; 2]) -> Option<[u8; 4]> {
    if image.width == 0 || image.height == 0 || !(0.0..=1.0).contains(&uv[0]) || !(0.0..=1.0).contains(&uv[1]) {
        return None;
    }
    let x = ((uv[0] * image.width as f32) as usize).min(image.width - 1);
    let y = ((uv[1] * image.height as f32) as usize).min(image.height - 1);
    let at = (y * image.width + x) * 4;
    image.rgba.get(at..at + 4).map(|texel| [texel[0], texel[1], texel[2], texel[3]])
}

/// The part of a triangle past the near plane, as a polygon of up to four corners.
fn clip(triangle: [Corner; 3], near: f32) -> Vec<Corner> {
    let mut out = Vec::with_capacity(4);
    for corner in 0..3 {
        let (here, next) = (triangle[corner], triangle[(corner + 1) % 3]);
        let (inside, next_inside) = (here.view[2] > near, next.view[2] > near);
        if inside {
            out.push(here);
        }
        if inside != next_inside {
            let t = (near - here.view[2]) / (next.view[2] - here.view[2]);
            let lerp2 = |a: [f32; 2], b: [f32; 2]| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            let view = math::add(here.view, math::scale(math::sub(next.view, here.view), t));
            out.push(Corner { view: [view[0], view[1], near.max(view[2])], uv: lerp2(here.uv, next.uv) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Primitive, Scene};

    /// A VRAM whose 8-bit page at (0, 0) holds texel `u` at every `(u, v)`, with CLUT row 500 mapping
    /// index `i` to the colour word `i` (and index 0 to transparent).
    fn vram() -> Vec<u16> {
        let mut vram = vec![0u16; VRAM_WIDTH * VRAM_HEIGHT];
        for v in 0..256 {
            for word in 0..128 {
                let (low, high) = ((2 * word) as u16, (2 * word + 1) as u16);
                vram[v * VRAM_WIDTH + word] = low | (high << 8);
            }
        }
        for index in 0..256 {
            vram[500 * VRAM_WIDTH + index] = index as u16 | if index == 31 { STP_BIT } else { 0 };
        }
        vram
    }

    fn quad(texture: Texture, colour: [u8; 4], blend: Blend) -> Primitive {
        Primitive { corners: [[-1.0, -1.0, 2.0], [1.0, -1.0, 2.0], [-1.0, 1.0, 2.0], [1.0, 1.0, 2.0]], texture, colour, blend, depth_test: true, id: None }
    }

    #[test]
    fn a_vram_texel_is_read_through_its_clut_and_shaded() {
        let texture = Texture::Vram { page: [0, 0], clut: [0, 500], depth: Depth::Eight, uvs: [[0, 0], [31, 0], [0, 31], [31, 31]] };
        let scene = Scene { vram: vram(), primitives: vec![quad(texture, [0x40; 4], Blend::Opaque)], ..Scene::default() };
        let frame = draw(&scene, &Camera::looking_down_z(32.0), 64, 64);
        // The quad spans pixels 16..48; pixel 47's texel is u = 30.5 -> 30, red channel 30 << 3 = 240,
        // halved by the 0x40 shade.
        assert_eq!(frame.pixel(47, 20)[0], 120);
        assert_eq!(frame.pixel(16, 20), [0, 0, 0, 255], "texel 0 is transparent");
        assert_eq!(frame.pixel(4, 4), [0, 0, 0, 255], "outside the quad");
    }

    #[test]
    fn semi_transparency_mixes_only_texels_with_bit_15() {
        let texture = Texture::Vram { page: [0, 0], clut: [0, 500], depth: Depth::Eight, uvs: [[31, 0], [31, 0], [31, 31], [31, 31]] };
        let mut scene = Scene { vram: vram(), background: [200, 200, 200, 255], ..Scene::default() };
        scene.primitives.push(quad(texture, [NEUTRAL_SHADE; 4], Blend::Psx(2)));
        let frame = draw(&scene, &Camera::looking_down_z(32.0), 64, 64);
        // Texel 31 has bit 15: 200 - (31 << 3) clamps to 0 in red; green and blue hold 200 - 0.
        assert_eq!(frame.pixel(32, 32), [0, 200, 200, 255]);
    }

    #[test]
    fn a_nearer_quad_hides_a_farther_one_and_the_near_plane_clips() {
        let mut far = quad(Texture::None, [255, 0, 0, 255], Blend::Opaque);
        far.corners = far.corners.map(|[x, y, _]| [x, y, 4.0]);
        let near = quad(Texture::None, [0, 255, 0, 255], Blend::Opaque);
        let scene = Scene { primitives: vec![near, far], ..Scene::default() };
        let frame = draw(&scene, &Camera::looking_down_z(32.0), 64, 64);
        assert_eq!(frame.pixel(32, 32), [0, 255, 0, 255]);

        let mut crossing = quad(Texture::None, [0, 0, 255, 255], Blend::Opaque);
        crossing.corners = [[-1.0, 0.0, -1.0], [1.0, 0.0, -1.0], [-1.0, 0.0, 3.0], [1.0, 0.0, 3.0]];
        crossing.corners = crossing.corners.map(|[x, _, z]| [x, 0.2, z]);
        let frame = draw(&Scene { primitives: vec![crossing], ..Scene::default() }, &Camera::looking_down_z(32.0), 64, 64);
        assert!(frame.rgba.chunks(4).any(|pixel| pixel[2] == 255), "the part past the near plane draws");
    }
}
