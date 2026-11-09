# ADR 0003: area 3D rendering

- Status: accepted
- Date: 2026-11-09
- Applies to: `rebof3-render` (new), `bof3-edit` (the area view's `3D` tab)

## Context

The area view draws an area's terrain the way the field renderer does: one textured `POLY_FT4`
per cell, the east and south walls, the tile's page, CLUT, depth, grey level and
semi-transparency (`rebof3_formats::map::draw`, read from `func_801530D0` and `func_80155560`).
On top of the terrain it draws the field data as overlays. It needs four things:

- the GPU's texturing rules: indexed texels looked up through a CLUT in VRAM, texel `0` drawn
  transparent, the STP bit choosing between opaque and the four blend modes, and the texel
  multiplied by the vertex colour with `0x80` as neutral;
- a depth buffer, so the free camera can turn the terrain any way;
- a pick, so a click names the quad or actor under it;
- CI and the tests run without a GPU and still check what is drawn.

The editor is gpui 0.2.2 (ADR 0001). gpui draws its own scene through blade (Vulkan on Linux and
Windows, Metal on macOS). It has no public way to put a foreign GPU surface into an element on
Linux or Windows: `Window::paint_surface` is `#[cfg(target_os = "macos")]` and takes a
`CVPixelBuffer`. What it can paint on every platform is an image: `Window::paint_image` takes an
`Arc<RenderImage>`, a BGRA frame it uploads to its sprite atlas.

## Decision

`rebof3-render` draws a `Scene` (VRAM words plus quads with PSX texture, colour and blend state)
for a `Camera` into an RGBA `Frame`, through one of two backends:

- **OpenGL ES 3.0 offscreen**, behind the `gles` cargo feature. It uses `glow` 0.18.0 for the GL
  calls and `khronos-egl` 6.0.0 with its `dynamic` feature. The `dynamic` feature loads
  `libEGL.so.1` (Mesa on Linux) or `libEGL.dll` (ANGLE on Windows) at run time, so the build
  needs no GL headers or libraries. The context is surfaceless (`EGL_PLATFORM_SURFACELESS_MESA`),
  with the default display and a pbuffer as the fallback. The VRAM goes up as an `R16UI` texture.
  The fragment shader fetches the texel and looks it up through the CLUT, as the GPU does. Texture
  coordinates are interpolated without perspective, like the GPU's affine mapping. Passes run in
  this order: opaque texels, then STP texels in their PSX blend mode, then the alpha overlays,
  then the overlays that ignore depth. The frame is read back with `glReadPixels`.
- **Software**, always built: a z-buffered rasterizer with the same texel fetch, blend modes,
  split order and near-plane clip.

`Renderer::new` tries OpenGL ES and falls back to software when no context opens, recording the
reason. A failed draw also falls back. The editor paints the frame with `paint_image` in a
`canvas`, and drops the frame it replaces from the atlas with `drop_image`. Picking
(`rebof3_render::pick`) is a CPU ray test over the scene. It does not depend on the backend, and
neither do the logs, the VRAM readback or the tests.

The editor turns the backend on through its own `gles` feature (on by default). The view has a
`Software renderer` switch, and shows which backend drew the frame and why OpenGL ES fell back.

## Evidence

- Parity: `crates/rebof3-render/tests/backends.rs` draws one scene through both backends. The
  scene has an opaque 8-bit quad at neutral shade, a shaded 4-bit quad, an 8-bit quad in blend
  mode 0 whose CLUT sets STP on every odd index, and an image with alpha. At 320 x 240 the frames
  differ in 39 of 76,800 pixels. The test bounds the share at 0.5% and skips when no EGL display
  opens. Blend modes 1 to 3 run through the same pass but have no parity case. No retail terrain
  quad sets the semi-transparency bit (`crates/rebof3-formats/tests/retail_area_draw.rs`).
- Coverage without a GPU: the editor's retail test draws every one of the 200 areas through the
  software path and checks the frame is not empty (`view/area/tests.rs`).
- Dependencies: `glow` and `khronos-egl` are MIT or Apache-2.0. Both were already in
  `Cargo.lock` through blade-graphics on its GLES targets.

## Alternatives

| Option | Why not |
| ------ | ------- |
| Share gpui's blade device and draw into its surface | gpui 0.2.2 exposes neither the device nor a surface element outside macOS. It would mean forking gpui. |
| wgpu offscreen, read back the same way | Adds a large dependency tree (naga, wgpu-hal) beside blade for the same readback design. Its WGSL has no integer texture fetch through a CLUT that is simpler than GLES's. |
| Draw quads as gpui paths | gpui paths have no texture coordinates, depth test or blend modes. Texturing would mean one image per tile, re-projected on the CPU each frame. |
| A separate native window with its own GL context | Splits the editor's window, focus and undo across two windows, and still needs a software path for CI. |
| Software only | Works everywhere and is the fallback. It is slow for large areas at window size, which OpenGL ES handles in a fraction of the time. |

## Consequences

- A frame costs a GPU readback of `width * height * 4` bytes. The view draws only when the camera,
  the layers, the pick, the size or the data change, never on hover.
- A machine with no EGL library, or a driver that refuses GLES 3.0, draws through the software
  path. The editor stays usable, but slower.
- The software path is the reference the tests check, so a GLES-only defect shows up as a parity
  failure in `backends.rs` on a machine with a GPU.
