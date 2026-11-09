//! The OpenGL ES renderer draws what the software renderer draws. Without an EGL display (a CI runner
//! with no GPU or Mesa) the comparison skips and says why.

use rebof3_render::{Backend, Blend, Camera, Depth, Image, NEUTRAL_SHADE, Primitive, Renderer, STP_BIT, Scene, Texture, VRAM_HEIGHT, VRAM_WIDTH, software};

/// A VRAM with an 8-bit page at (64, 0) whose texel at `(u, v)` is `(u + v) & 0xFF`, a 4-bit page at
/// (128, 0) whose texel is `u & 15`, and a CLUT row 480 mapping index `i` to a colour with bit 15 on
/// for odd `i`.
fn vram() -> Vec<u16> {
    let mut vram = vec![0u16; VRAM_WIDTH * VRAM_HEIGHT];
    for v in 0..256 {
        for word in 0..128 {
            let texel = |u: usize| ((u + v) & 0xFF) as u16;
            vram[v * VRAM_WIDTH + 64 + word] = texel(2 * word) | (texel(2 * word + 1) << 8);
        }
        for word in 0..64 {
            vram[v * VRAM_WIDTH + 128 + word] = 0x3210;
        }
    }
    for index in 0..256u16 {
        let colour = (index & 0x1F) | ((index >> 2) & 0x1F) << 5 | ((31 - (index & 0x1F)) << 10);
        vram[480 * VRAM_WIDTH + usize::from(index)] = colour | if index & 1 == 1 { STP_BIT } else { 0 };
    }
    vram
}

fn quad(corners: [[f32; 3]; 4], texture: Texture, colour: [u8; 4], blend: Blend) -> Primitive {
    Primitive { corners, texture, colour, blend, depth_test: true, id: None }
}

fn scene() -> Scene {
    let ground = [[-400.0, 0.0, -400.0], [400.0, 0.0, -400.0], [-400.0, 0.0, 400.0], [400.0, 0.0, 400.0]];
    let wall = [[-200.0, 150.0, 0.0], [200.0, 150.0, 0.0], [-200.0, 0.0, 0.0], [200.0, 0.0, 0.0]];
    let glass = [[-300.0, 100.0, 100.0], [0.0, 100.0, 100.0], [-300.0, 0.0, 100.0], [0.0, 0.0, 100.0]];
    let sign = [[100.0, 120.0, 150.0], [250.0, 120.0, 150.0], [100.0, 20.0, 150.0], [250.0, 20.0, 150.0]];
    let eight = |uvs| Texture::Vram { page: [64, 0], clut: [0, 480], depth: Depth::Eight, uvs };
    let four = Texture::Vram { page: [128, 0], clut: [16, 480], depth: Depth::Four, uvs: [[0, 0], [63, 0], [0, 63], [63, 63]] };
    let image = Image { width: 2, height: 2, rgba: vec![255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 255, 255, 255, 255] };
    Scene {
        vram: vram(),
        images: vec![image],
        background: [20, 30, 40, 255],
        primitives: vec![
            quad(ground, eight([[0, 0], [255, 0], [0, 255], [255, 255]]), [NEUTRAL_SHADE; 4], Blend::Opaque),
            quad(wall, four, [0x60, 0x80, 0xA0, 0xFF], Blend::Opaque),
            quad(glass, eight([[10, 10], [90, 10], [10, 90], [90, 90]]), [NEUTRAL_SHADE; 4], Blend::Psx(0)),
            quad(sign, Texture::Image { image: 0, uvs: [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]] }, [255; 4], Blend::Alpha),
        ],
    }
}

#[test]
fn gles_draws_what_software_draws() {
    let mut renderer = Renderer::new(true);
    if renderer.backend() != Backend::Gles {
        eprintln!("skipping: {}", renderer.fallback.as_deref().unwrap_or("no OpenGL ES"));
        return;
    }
    let scene = scene();
    let camera = Camera::orbit([0.0, 40.0, 0.0], 0.6, 0.5, 900.0, 1.0);
    let (width, height) = (320, 240);
    let gles = renderer.draw(&scene, &camera, width, height);
    assert_eq!(renderer.backend(), Backend::Gles, "{:?}", renderer.fallback);
    let soft = software::draw(&scene, &camera, width, height);

    let differing = gles.rgba.chunks(4).zip(soft.rgba.chunks(4)).filter(|(left, right)| left.iter().zip(right.iter()).any(|(a, b)| a.abs_diff(*b) > 2)).count();
    if let Some(dir) = std::env::var_os("RENDER_DUMP") {
        let dir = std::path::PathBuf::from(dir);
        for (name, frame) in [("gles", &gles), ("soft", &soft)] {
            let mut ppm = format!("P6 {width} {height} 255\n").into_bytes();
            ppm.extend(frame.rgba.chunks(4).flat_map(|pixel| [pixel[0], pixel[1], pixel[2]]));
            std::fs::write(dir.join(format!("{name}.ppm")), ppm).unwrap();
        }
    }
    let share = differing as f32 / (width * height) as f32;
    eprintln!("{differing} of {} pixels differ", width * height);
    assert!(share < 0.005, "{differing} of {} pixels differ", width * height);
    let background = gles.rgba.chunks(4).filter(|pixel| pixel[..3] == [20, 30, 40]).count();
    assert!(background < width * height / 2, "the scene draws over most of the frame");
}
