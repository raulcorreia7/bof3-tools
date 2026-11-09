//! The OpenGL ES 3.0 renderer: an offscreen EGL context, a framebuffer object of the frame's size,
//! and the GPU's texel rules in a fragment shader. The frame is read back into memory, so the
//! window toolkit shows it like any image.
//!
//! EGL is loaded at run time (`libEGL.so.1` on Linux, ANGLE's `libEGL.dll` on Windows); a missing
//! library, display or context is an error and the caller draws in software. The display is Mesa's
//! surfaceless platform when there is one, else the default display.
//!
//! The shader reads the VRAM as a 1024 x 512 `R16UI` texture and looks each texel up through its CLUT
//! row, as [`crate::software::fetch`] does. Texture coordinates are affine in screen space like the
//! GPU's: the vertex stage multiplies them by the clip `w` and the fragment stage divides by the
//! interpolated `w`. Semi-transparent texels use the blend equations of the GPU's four modes.

use std::sync::Arc;

use glow::HasContext;
use khronos_egl as egl;

use crate::{Blend, Camera, Depth, Frame, Primitive, Scene, Texture, VRAM_HEIGHT, VRAM_WIDTH, draw_order};

/// Mesa's surfaceless EGL platform (`EGL_MESA_platform_surfaceless`).
const PLATFORM_SURFACELESS_MESA: egl::Enum = 0x31DD;
/// The depth the projection maps to the far plane.
const FAR: f32 = 1.0e6;
/// Floats per vertex: view position 3, texture coordinate 2, colour 4, texture kind, page 2, depth,
/// CLUT 2, semi-transparency.
const VERTEX_FLOATS: usize = 16;

const VERTEX_SHADER: &str = r#"#version 300 es
precision highp float;
uniform vec4 u_projection;
in vec3 a_view;
in vec2 a_uv;
in vec4 a_colour;
in vec4 a_texture;
in vec3 a_clut;
out vec2 v_uvw;
out float v_w;
flat out vec4 v_colour;
flat out ivec4 v_texture;
flat out ivec2 v_clut;
flat out int v_semi;
void main() {
    float z = a_view.z;
    gl_Position = vec4(u_projection.x * a_view.x, u_projection.y * a_view.y, u_projection.z * z + u_projection.w, z);
    v_uvw = a_uv * z;
    v_w = z;
    v_colour = a_colour;
    v_texture = ivec4(a_texture);
    v_clut = ivec2(a_clut.xy);
    v_semi = int(a_clut.z);
}
"#;

const FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp usampler2D;
uniform usampler2D u_vram;
uniform sampler2D u_image;
uniform ivec2 u_image_size;
// 0: opaque texels only, 1: semi-transparent texels only, 2: everything.
uniform int u_pass;
in vec2 v_uvw;
in float v_w;
flat in vec4 v_colour;
flat in ivec4 v_texture;
flat in ivec2 v_clut;
flat in int v_semi;
out vec4 o_colour;
void main() {
    vec2 uv = v_uvw / v_w;
    ivec4 colour = ivec4(v_colour * 255.0 + 0.5);
    bool mixed = v_semi == 1;
    vec4 result;
    if (v_texture.x == 0) {
        result = v_colour;
    } else if (v_texture.x == 1) {
        ivec2 texel = clamp(ivec2(uv), ivec2(0), ivec2(255));
        int depth = v_texture.w;
        int per_word = depth == 4 ? 4 : (depth == 8 ? 2 : 1);
        ivec2 at = ivec2((v_texture.y + texel.x / per_word) % 1024, (v_texture.z + texel.y) % 512);
        uint word = texelFetch(u_vram, at, 0).r;
        if (depth != 15) {
            int bits = 16 / per_word;
            uint index = (word >> uint((texel.x % per_word) * bits)) & uint((1 << bits) - 1);
            word = texelFetch(u_vram, ivec2((v_clut.x + int(index)) % 1024, v_clut.y % 512), 0).r;
        }
        if (word == 0u) discard;
        ivec3 rgb = ivec3(int(word & 31u), int((word >> 5) & 31u), int((word >> 10) & 31u)) << 3;
        rgb = min(rgb * colour.rgb / 128, ivec3(255));
        mixed = v_semi == 1 && (word & 0x8000u) != 0u;
        result = vec4(vec3(rgb) / 255.0, 1.0);
    } else {
        if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) discard;
        ivec2 at = min(ivec2(uv * vec2(u_image_size)), u_image_size - 1);
        vec4 texel = texelFetch(u_image, at, 0);
        float alpha = floor(texel.a * 255.0 + 0.5) * float(colour.a) / 255.0;
        alpha = floor(alpha) / 255.0;
        if (alpha <= 0.0) discard;
        result = vec4(texel.rgb, alpha);
    }
    if (u_pass == 0 && mixed) discard;
    if (u_pass == 1 && !mixed) discard;
    o_colour = result;
}
"#;

/// An open context and what it keeps between frames.
pub struct Gles {
    egl: egl::DynamicInstance<egl::EGL1_5>,
    display: egl::Display,
    context: egl::Context,
    surface: Option<egl::Surface>,
    gl: Arc<glow::Context>,
    program: glow::Program,
    vram: glow::Texture,
    /// The VRAM words last uploaded.
    uploaded: Vec<u16>,
    target: Option<(usize, usize, glow::Framebuffer, glow::Renderbuffer, glow::Renderbuffer)>,
    buffer: glow::Buffer,
    array: glow::VertexArray,
}

impl Gles {
    /// Opens an offscreen OpenGL ES 3.0 context.
    pub fn new() -> Result<Self, String> {
        // SAFETY: the library is the platform's EGL, loaded by name; its entry points are the EGL 1.5
        // functions the binding declares.
        let egl = unsafe { load() }.map_err(|error| format!("EGL is not available: {error}"))?;
        let display = open_display(&egl)?;
        egl.initialize(display).map_err(|error| format!("EGL does not initialize: {error}"))?;
        egl.bind_api(egl::OPENGL_ES_API).map_err(|error| format!("EGL has no OpenGL ES: {error}"))?;
        let attributes = [egl::RENDERABLE_TYPE, egl::OPENGL_ES3_BIT, egl::SURFACE_TYPE, egl::PBUFFER_BIT, egl::NONE];
        let config = egl
            .choose_first_config(display, &attributes)
            .map_err(|error| error.to_string())?
            .ok_or("EGL has no OpenGL ES 3 configuration")?;
        let context = egl
            .create_context(display, config, None, &[egl::CONTEXT_MAJOR_VERSION, 3, egl::NONE])
            .map_err(|error| format!("EGL makes no OpenGL ES 3 context: {error}"))?;
        let surface = if egl.make_current(display, None, None, Some(context)).is_ok() {
            None
        } else {
            let surface = egl
                .create_pbuffer_surface(display, config, &[egl::WIDTH, 16, egl::HEIGHT, 16, egl::NONE])
                .map_err(|error| format!("EGL makes no pbuffer: {error}"))?;
            egl.make_current(display, Some(surface), Some(surface), Some(context))
                .map_err(|error| format!("EGL cannot make the context current: {error}"))?;
            Some(surface)
        };
        // SAFETY: the context is current on this thread, and every name resolves through EGL.
        let gl = unsafe {
            glow::Context::from_loader_function(|name| {
                egl.get_proc_address(name).map_or(std::ptr::null(), |function| function as *const _)
            })
        };
        // SAFETY: the context is current; every object made here belongs to it.
        unsafe { Self::build(egl, display, context, surface, Arc::new(gl)) }
    }

    /// Compiles the program and makes the VRAM texture and the vertex buffer.
    unsafe fn build(
        egl: egl::DynamicInstance<egl::EGL1_5>,
        display: egl::Display,
        context: egl::Context,
        surface: Option<egl::Surface>,
        gl: Arc<glow::Context>,
    ) -> Result<Self, String> {
        // SAFETY: the caller made the context current.
        unsafe {
            let program = gl.create_program()?;
            for (kind, source) in [(glow::VERTEX_SHADER, VERTEX_SHADER), (glow::FRAGMENT_SHADER, FRAGMENT_SHADER)] {
                let shader = gl.create_shader(kind)?;
                gl.shader_source(shader, source);
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    return Err(format!("a shader does not compile: {}", gl.get_shader_info_log(shader)));
                }
                gl.attach_shader(program, shader);
            }
            for (location, name) in ["a_view", "a_uv", "a_colour", "a_texture", "a_clut"].iter().enumerate() {
                gl.bind_attrib_location(program, location as u32, name);
            }
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                return Err(format!("the program does not link: {}", gl.get_program_info_log(program)));
            }
            let vram = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(vram));
            for parameter in [glow::TEXTURE_MIN_FILTER, glow::TEXTURE_MAG_FILTER] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, parameter, glow::NEAREST as i32);
            }
            let zero = vec![0u8; VRAM_WIDTH * VRAM_HEIGHT * 2];
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::R16UI as i32,
                VRAM_WIDTH as i32,
                VRAM_HEIGHT as i32,
                0,
                glow::RED_INTEGER,
                glow::UNSIGNED_SHORT,
                glow::PixelUnpackData::Slice(Some(&zero)),
            );
            let buffer = gl.create_buffer()?;
            let array = gl.create_vertex_array()?;
            gl.bind_vertex_array(Some(array));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
            let stride = (VERTEX_FLOATS * 4) as i32;
            let mut offset = 0;
            for (location, size) in [3, 2, 4, 4, 3].into_iter().enumerate() {
                gl.enable_vertex_attrib_array(location as u32);
                gl.vertex_attrib_pointer_f32(location as u32, size, glow::FLOAT, false, stride, offset);
                offset += size * 4;
            }
            Ok(Self {
                egl,
                display,
                context,
                surface,
                gl,
                program,
                vram,
                uploaded: vec![0; VRAM_WIDTH * VRAM_HEIGHT],
                target: None,
                buffer,
                array,
            })
        }
    }

    /// Draws `scene` through `camera` into a `width` x `height` frame.
    pub fn draw(&mut self, scene: &Scene, camera: &Camera, width: usize, height: usize) -> Result<Frame, String> {
        self.egl
            .make_current(self.display, self.surface, self.surface, Some(self.context))
            .map_err(|error| format!("EGL cannot make the context current: {error}"))?;
        // SAFETY: the context is current, and every object used was made in it.
        unsafe { self.draw_current(scene, camera, width.max(1), height.max(1)) }
    }

    unsafe fn draw_current(
        &mut self,
        scene: &Scene,
        camera: &Camera,
        width: usize,
        height: usize,
    ) -> Result<Frame, String> {
        // SAFETY: the caller made the context current.
        unsafe {
            let gl = Arc::clone(&self.gl);
            self.bind_target(width, height)?;
            if scene.vram.len() == VRAM_WIDTH * VRAM_HEIGHT && scene.vram != self.uploaded {
                gl.bind_texture(glow::TEXTURE_2D, Some(self.vram));
                let bytes: Vec<u8> = scene.vram.iter().flat_map(|word| word.to_le_bytes()).collect();
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    VRAM_WIDTH as i32,
                    VRAM_HEIGHT as i32,
                    glow::RED_INTEGER,
                    glow::UNSIGNED_SHORT,
                    glow::PixelUnpackData::Slice(Some(&bytes)),
                );
                self.uploaded.clone_from(&scene.vram);
            }
            let textures = self.upload_images(scene)?;
            let images: Vec<(glow::Texture, [i32; 2])> = textures
                .iter()
                .zip(&scene.images)
                .map(|(&texture, image)| (texture, [image.width.max(1) as i32, image.height.max(1) as i32]))
                .collect();

            gl.viewport(0, 0, width as i32, height as i32);
            let [red, green, blue, _] = scene.background.map(|channel| f32::from(channel) / 255.0);
            gl.clear_color(red, green, blue, 1.0);
            gl.clear_depth_f32(1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            gl.use_program(Some(self.program));
            let focal = camera.focal * width as f32 / camera.frame_width;
            let (near, far) = (camera.near, FAR);
            gl.uniform_4_f32(
                gl.get_uniform_location(self.program, "u_projection").as_ref(),
                2.0 * focal / width as f32,
                -2.0 * focal / height as f32,
                (far + near) / (far - near),
                -2.0 * far * near / (far - near),
            );
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.vram));
            gl.uniform_1_i32(gl.get_uniform_location(self.program, "u_vram").as_ref(), 0);
            gl.uniform_1_i32(gl.get_uniform_location(self.program, "u_image").as_ref(), 1);
            gl.bind_vertex_array(Some(self.array));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.buffer));
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LESS);

            let order = draw_order(scene, camera);
            let pass = gl.get_uniform_location(self.program, "u_pass");
            // The opaque texels of every depth-tested primitive but the alpha overlays.
            let solid: Vec<&Primitive> = order
                .iter()
                .map(|&index| &scene.primitives[index])
                .filter(|primitive| primitive.depth_test && primitive.blend != Blend::Alpha)
                .collect();
            gl.disable(glow::BLEND);
            gl.depth_mask(true);
            gl.uniform_1_i32(pass.as_ref(), 0);
            self.draw_run(&solid, camera, &images);
            // The semi-transparent texels, far to near, one run per mode.
            gl.uniform_1_i32(pass.as_ref(), 1);
            gl.depth_mask(false);
            gl.enable(glow::BLEND);
            for run in runs(&solid, |primitive| primitive.blend) {
                let Blend::Psx(mode) = run[0].blend else { continue };
                psx_blend(&gl, mode);
                self.draw_run(&run, camera, &images);
            }
            // The overlays: alpha over the frame, then the ones drawn on top.
            gl.uniform_1_i32(pass.as_ref(), 2);
            gl.blend_equation(glow::FUNC_ADD);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            let alpha: Vec<&Primitive> = order
                .iter()
                .map(|&index| &scene.primitives[index])
                .filter(|primitive| primitive.depth_test && primitive.blend == Blend::Alpha)
                .collect();
            self.draw_run(&alpha, camera, &images);
            gl.disable(glow::DEPTH_TEST);
            let top: Vec<&Primitive> =
                order.iter().map(|&index| &scene.primitives[index]).filter(|primitive| !primitive.depth_test).collect();
            for run in runs(&top, |primitive| primitive.blend) {
                match run[0].blend {
                    Blend::Psx(mode) => psx_blend(&gl, mode),
                    Blend::Alpha => {
                        gl.blend_equation(glow::FUNC_ADD);
                        gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
                    }
                    Blend::Opaque => gl.disable(glow::BLEND),
                }
                self.draw_run(&run, camera, &images);
                gl.enable(glow::BLEND);
            }

            let mut rgba = vec![0u8; width * height * 4];
            gl.read_pixels(
                0,
                0,
                width as i32,
                height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut rgba)),
            );
            for texture in textures {
                gl.delete_texture(texture);
            }
            let error = gl.get_error();
            if error != glow::NO_ERROR {
                return Err(format!("OpenGL ES reports error {error:#06X}"));
            }
            // OpenGL's first row is the bottom one.
            let row = width * 4;
            let mut flipped = Vec::with_capacity(rgba.len());
            for line in rgba.chunks(row).rev() {
                flipped.extend_from_slice(line);
            }
            for pixel in flipped.chunks_mut(4) {
                pixel[3] = 0xFF;
            }
            Ok(Frame { width, height, rgba: flipped })
        }
    }

    /// Makes the framebuffer object of the frame's size current.
    unsafe fn bind_target(&mut self, width: usize, height: usize) -> Result<(), String> {
        // SAFETY: the caller made the context current.
        unsafe {
            let gl = &self.gl;
            if let Some((w, h, framebuffer, _, _)) = self.target
                && (w, h) == (width, height)
            {
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
                return Ok(());
            }
            if let Some((_, _, framebuffer, colour, depth)) = self.target.take() {
                gl.delete_framebuffer(framebuffer);
                gl.delete_renderbuffer(colour);
                gl.delete_renderbuffer(depth);
            }
            let framebuffer = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
            let colour = gl.create_renderbuffer()?;
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(colour));
            gl.renderbuffer_storage(glow::RENDERBUFFER, glow::RGBA8, width as i32, height as i32);
            gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::RENDERBUFFER, Some(colour));
            let depth = gl.create_renderbuffer()?;
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth));
            gl.renderbuffer_storage(glow::RENDERBUFFER, glow::DEPTH_COMPONENT24, width as i32, height as i32);
            gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::DEPTH_ATTACHMENT, glow::RENDERBUFFER, Some(depth));
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                return Err("the framebuffer object is incomplete".into());
            }
            self.target = Some((width, height, framebuffer, colour, depth));
            Ok(())
        }
    }

    /// One texture per scene image, for this frame.
    unsafe fn upload_images(&self, scene: &Scene) -> Result<Vec<glow::Texture>, String> {
        // SAFETY: the caller made the context current.
        unsafe {
            let gl = &self.gl;
            let mut textures = Vec::with_capacity(scene.images.len());
            for image in &scene.images {
                let texture = gl.create_texture()?;
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                for parameter in [glow::TEXTURE_MIN_FILTER, glow::TEXTURE_MAG_FILTER] {
                    gl.tex_parameter_i32(glow::TEXTURE_2D, parameter, glow::NEAREST as i32);
                }
                let (width, height) = (image.width.max(1), image.height.max(1));
                let mut bytes = image.rgba.clone();
                bytes.resize(width * height * 4, 0);
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    width as i32,
                    height as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&bytes)),
                );
                textures.push(texture);
            }
            Ok(textures)
        }
    }

    /// Draws primitives in order, binding each image's texture for its own run.
    unsafe fn draw_run(&self, primitives: &[&Primitive], camera: &Camera, images: &[(glow::Texture, [i32; 2])]) {
        // SAFETY: the caller made the context current and bound the program and the vertex array.
        unsafe {
            let gl = &self.gl;
            let image_of = |primitive: &&Primitive| match primitive.texture {
                Texture::Image { image, .. } => Some(image),
                _ => None,
            };
            for run in runs(primitives, image_of) {
                if let Some(image) = image_of(&run[0]) {
                    let Some(&(texture, [width, height])) = images.get(image) else { continue };
                    gl.active_texture(glow::TEXTURE1);
                    gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                    gl.active_texture(glow::TEXTURE0);
                    gl.uniform_2_i32(gl.get_uniform_location(self.program, "u_image_size").as_ref(), width, height);
                }
                let mut floats = Vec::with_capacity(run.len() * 6 * VERTEX_FLOATS);
                for primitive in &run {
                    for corner in [0, 1, 2, 1, 2, 3] {
                        vertex(&mut floats, primitive, camera, corner);
                    }
                }
                let bytes: Vec<u8> = floats.iter().flat_map(|value| value.to_le_bytes()).collect();
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &bytes, glow::STREAM_DRAW);
                gl.draw_arrays(glow::TRIANGLES, 0, (run.len() * 6) as i32);
            }
        }
    }
}

impl Drop for Gles {
    fn drop(&mut self) {
        let _ = self.egl.make_current(self.display, None, None, None);
        let _ = self.egl.destroy_context(self.display, self.context);
        if let Some(surface) = self.surface {
            let _ = self.egl.destroy_surface(self.display, surface);
        }
    }
}

/// Loads the platform's EGL library.
unsafe fn load() -> Result<egl::DynamicInstance<egl::EGL1_5>, String> {
    let names: &[&str] = if cfg!(windows) { &["libEGL.dll"] } else { &["libEGL.so.1", "libEGL.so"] };
    let mut last = String::from("no library name");
    for name in names {
        // SAFETY: the named library is the platform's EGL; the caller accepts that contract.
        match unsafe { egl::DynamicInstance::<egl::EGL1_5>::load_required_from_filename(name) } {
            Ok(instance) => return Ok(instance),
            Err(error) => last = format!("{name}: {error}"),
        }
    }
    Err(last)
}

/// Mesa's surfaceless display when the platform offers it, else the default display.
fn open_display(egl: &egl::DynamicInstance<egl::EGL1_5>) -> Result<egl::Display, String> {
    let extensions = egl.query_string(None, egl::EXTENSIONS).map(|text| text.to_string_lossy().into_owned());
    if extensions.is_ok_and(|text| text.contains("EGL_MESA_platform_surfaceless")) {
        // SAFETY: the surfaceless platform takes no native display.
        if let Ok(display) =
            unsafe { egl.get_platform_display(PLATFORM_SURFACELESS_MESA, egl::DEFAULT_DISPLAY, &[egl::ATTRIB_NONE]) }
        {
            return Ok(display);
        }
    }
    // SAFETY: the default display needs no native handle.
    unsafe { egl.get_display(egl::DEFAULT_DISPLAY) }.ok_or_else(|| "EGL has no display".into())
}

/// Sets the blend equation of the GPU's semi-transparency mode (psx-spx, "GPU Texpage Attribute").
unsafe fn psx_blend(gl: &glow::Context, mode: u8) {
    // SAFETY: the caller made the context current.
    unsafe {
        match mode & 3 {
            0 => {
                gl.blend_equation(glow::FUNC_ADD);
                gl.blend_color(0.5, 0.5, 0.5, 0.5);
                gl.blend_func(glow::CONSTANT_COLOR, glow::CONSTANT_COLOR);
            }
            1 => {
                gl.blend_equation(glow::FUNC_ADD);
                gl.blend_func(glow::ONE, glow::ONE);
            }
            2 => {
                gl.blend_equation(glow::FUNC_REVERSE_SUBTRACT);
                gl.blend_func(glow::ONE, glow::ONE);
            }
            _ => {
                gl.blend_equation(glow::FUNC_ADD);
                gl.blend_color(0.25, 0.25, 0.25, 0.25);
                gl.blend_func(glow::CONSTANT_COLOR, glow::ONE);
            }
        }
    }
}

/// Splits `items` into runs whose `key` is the same.
fn runs<'a, K: PartialEq>(items: &[&'a Primitive], key: impl Fn(&&'a Primitive) -> K) -> Vec<Vec<&'a Primitive>> {
    let mut out: Vec<Vec<&'a Primitive>> = Vec::new();
    for item in items {
        match out.last_mut() {
            Some(run) if key(&run[0]) == key(item) => run.push(item),
            _ => out.push(vec![item]),
        }
    }
    out
}

/// Appends one corner's vertex.
fn vertex(floats: &mut Vec<f32>, primitive: &Primitive, camera: &Camera, corner: usize) {
    floats.extend(camera.view(primitive.corners[corner]));
    let (uv, kind, page, clut, depth) = match primitive.texture {
        Texture::None => ([0.0; 2], 0.0, [0.0; 2], [0.0; 2], 0.0),
        Texture::Vram { page, clut, depth, uvs } => {
            let bits = match depth {
                Depth::Four => 4.0,
                Depth::Eight => 8.0,
                Depth::Fifteen => 15.0,
            };
            let [u, v] = uvs[corner];
            ([f32::from(u), f32::from(v)], 1.0, page.map(f32::from), clut.map(f32::from), bits)
        }
        Texture::Image { uvs, .. } => (uvs[corner], 2.0, [0.0; 2], [0.0; 2], 0.0),
    };
    floats.extend(uv);
    floats.extend(primitive.colour.map(|channel| f32::from(channel) / 255.0));
    floats.extend([kind, page[0], page[1], depth]);
    floats.extend(clut);
    floats.push(if matches!(primitive.blend, Blend::Psx(_)) { 1.0 } else { 0.0 });
}
