use crate::math::*;

pub fn disp_tri_indices(power: i32) -> Vec<[usize; 3]> {
    let w = (1usize << power) + 1;
    let mut out = Vec::with_capacity(2 * (w - 1) * (w - 1));
    for y in 0..w - 1 {
        for x in 0..w - 1 {
            let v = y * w + x;
            if v % 2 == 1 {
                out.push([v, v + w, v + 1]);
                out.push([v + 1, v + w, v + w + 1]);
            } else {
                out.push([v, v + w, v + w + 1]);
                out.push([v, v + w + 1, v + 1]);
            }
        }
    }
    out
}

pub fn corners_from_face(face: &[Vec3], start: Vec3) -> [Vec3; 4] {
    let mut best = 0;
    let mut bestd = f64::MAX;
    for (i, p) in face.iter().enumerate().take(4) {
        let d = (*p - start).length_sq();
        if d < bestd {
            bestd = d;
            best = i;
        }
    }
    [
        face[best],
        face[(best + 1) % 4],
        face[(best + 2) % 4],
        face[(best + 3) % 4],
    ]
}

pub fn flat_position(corners: &[Vec3; 4], size: usize, i: usize, j: usize) -> Vec3 {
    let interval = (size - 1) as f64;
    let e0 = corners[0] + (corners[1] - corners[0]) * (i as f64 / interval);
    let e1 = corners[3] + (corners[2] - corners[3]) * (i as f64 / interval);
    e0 + (e1 - e0) * (j as f64 / interval)
}

pub fn disp_lightmap_size(corners: &[Vec3; 4], lmvecs: &[[f32; 4]; 2]) -> [i32; 2] {
    let scale = |k: usize| {
        let l = (lmvecs[k][0] as f64).powi(2)
            + (lmvecs[k][1] as f64).powi(2)
            + (lmvecs[k][2] as f64).powi(2);
        if l > 0.0 { l.sqrt() } else { 1.0 / 16.0 }
    };
    let col = (corners[3] - corners[0])
        .length()
        .max((corners[2] - corners[1]).length());
    let row = (corners[1] - corners[0])
        .length()
        .max((corners[2] - corners[3]).length());
    let s = ((col * scale(0)) - 0.001).ceil().max(1.0) as i32;
    let t = ((row * scale(1)) - 0.001).ceil().max(1.0) as i32;
    [s, t]
}

pub fn disp_vertex_luxel(power: i32, size: [i32; 2], i: usize, j: usize) -> (f64, f64) {
    let q = ((1usize << power) as f64).max(1.0);
    (j as f64 / q * size[0] as f64, i as f64 / q * size[1] as f64)
}

pub fn disp_param_locate(power: i32, fi: f64, fj: f64) -> (usize, [f64; 3]) {
    let w = (1usize << power) + 1;
    let q = w - 1;
    let r = (fi.clamp(0.0, 1.0) * q as f64).min(q as f64 - 1e-9);
    let c = (fj.clamp(0.0, 1.0) * q as f64).min(q as f64 - 1e-9);
    let (ri, ci) = (r.floor() as usize, c.floor() as usize);
    let (v, u) = (r - ri as f64, c - ci as f64);
    let vert = ri * w + ci;
    let quad = ri * q + ci;
    if vert % 2 == 1 {
        if v + u <= 1.0 {
            (quad * 2, [1.0 - v - u, v, u])
        } else {
            (quad * 2 + 1, [1.0 - v, 1.0 - u, v + u - 1.0])
        }
    } else if v >= u {
        (quad * 2, [1.0 - v, v - u, u])
    } else {
        (quad * 2 + 1, [1.0 - u, v, u - v])
    }
}

pub fn lightmap_sample_positions(power: i32, lm_size: [i32; 2]) -> Vec<u8> {
    let mut out = Vec::new();
    for t in 0..=lm_size[1] {
        for s in 0..=lm_size[0] {
            let fi = t as f64 / lm_size[1].max(1) as f64;
            let fj = s as f64 / lm_size[0].max(1) as f64;
            let (mut ti, bary) = disp_param_locate(power, fi, fj);
            while ti >= 255 {
                out.push(255u8);
                ti -= 255;
            }
            out.push(ti as u8);
            for b in bary {
                out.push((b * 255.9).clamp(0.0, 255.0) as u8);
            }
        }
    }
    out
}
