//! The little vector and matrix arithmetic the cameras and the rasterizer share.

/// A row-major 3x3 matrix.
pub type Matrix = [[f32; 3]; 3];

/// One full turn in the GTE's angle units (PsyQ `ONE`, 4096).
pub const TURN: f32 = 4096.0;

pub fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn scale(a: [f32; 3], by: f32) -> [f32; 3] {
    [a[0] * by, a[1] * by, a[2] * by]
}

pub fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn normalize(a: [f32; 3]) -> [f32; 3] {
    let length = dot(a, a).sqrt();
    if length == 0.0 { a } else { scale(a, 1.0 / length) }
}

/// `m * v`.
pub fn apply(m: &Matrix, v: [f32; 3]) -> [f32; 3] {
    [dot(m[0], v), dot(m[1], v), dot(m[2], v)]
}

/// `a * b`.
pub fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    std::array::from_fn(|row| std::array::from_fn(|column| (0..3).map(|k| a[row][k] * b[k][column]).sum()))
}

/// The rotation PsyQ's `RotMatrix` builds from GTE angles (`RotMatrix`, `exe/slus_004_22@0x80179738`,
/// the PsyQ 3.7 `libgte` object): `Rx * Ry * Rz`, so a vector turns about z first, then y, then x.
pub fn rot_matrix(angle: [i32; 3]) -> Matrix {
    let [x, y, z] = angle.map(|units| units as f32 / TURN * std::f32::consts::TAU);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    multiply(&multiply(&rx, &ry), &rz)
}

/// The barycentric weights of `point` in the screen triangle `a`, `b`, `c`, when it lies inside or
/// on an edge; `None` for a point outside or a degenerate triangle.
pub fn barycentric(a: [f32; 2], b: [f32; 2], c: [f32; 2], point: [f32; 2]) -> Option<[f32; 3]> {
    let area = edge(a, b, c);
    if area.abs() < f32::EPSILON {
        return None;
    }
    let wa = edge(b, c, point) / area;
    let wb = edge(c, a, point) / area;
    let wc = edge(a, b, point) / area;
    (wa >= 0.0 && wb >= 0.0 && wc >= 0.0).then_some([wa, wb, wc])
}

/// Twice the signed area of `a`, `b`, `c`.
pub fn edge(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rot_matrix_turns_about_z_then_x() {
        // A quarter turn about z takes x to y; a quarter turn about x then takes y to z.
        let m = rot_matrix([1024, 0, 1024]);
        let v = apply(&m, [1.0, 0.0, 0.0]);
        assert!((v[0]).abs() < 1e-5 && (v[1]).abs() < 1e-5 && (v[2] - 1.0).abs() < 1e-5, "{v:?}");
    }
}
