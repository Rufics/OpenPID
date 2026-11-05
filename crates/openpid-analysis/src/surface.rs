//! A heatmap grid as a 3D surface (freq × throttle × amplitude, DESIGN §6.6):
//! an orbiting orthographic camera and a shaded triangle mesh, all in software
//! and dependency-free. The GUI turns [`SurfaceMesh`] into its own mesh type
//! and draws the overlay lines with [`Camera::project`]; the CLI has no use
//! for it, so nothing here touches pixels.
//!
//! Model space: `x` (columns) and `y` (rows) of the grid are mapped to
//! `-0.5..=0.5`, `z` to `0..=height` as a fraction of the colour maximum
//! (values above it are flattened at the ceiling, as the heatmap saturates).

use openpid_core::analysis::Grid2D;

use crate::render::{robust_max, viridis};

/// Orbiting orthographic camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Rotation about the vertical axis, radians.
    pub yaw: f32,
    /// Tilt above the base plane, radians: `0` is edge-on, `π/2` straight down.
    pub pitch: f32,
    /// Screen pixels per model unit.
    pub zoom: f32,
    /// Screen position of the model origin, pixels.
    pub center: [f32; 2],
    /// Height of the colour maximum in model units.
    pub height: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            yaw: -0.6,
            pitch: 0.55,
            zoom: 400.0,
            center: [0.0, 0.0],
            height: 0.35,
        }
    }
}

impl Camera {
    /// Lowest tilt that still shows the base plane.
    pub const MIN_PITCH: f32 = 0.05;
    /// Highest tilt: straight down, the 2D heatmap.
    pub const MAX_PITCH: f32 = std::f32::consts::FRAC_PI_2;

    /// Clamp the tilt and keep the yaw in `-π..=π`.
    pub fn normalise(&mut self) {
        self.pitch = self.pitch.clamp(Self::MIN_PITCH, Self::MAX_PITCH);
        self.yaw = self.yaw.rem_euclid(std::f32::consts::TAU);
        if self.yaw > std::f32::consts::PI {
            self.yaw -= std::f32::consts::TAU;
        }
        self.zoom = self.zoom.clamp(20.0, 20_000.0);
        self.height = self.height.clamp(0.0, 2.0);
    }

    /// Screen position of a model-space point and its depth (larger = farther).
    pub fn project(&self, p: [f32; 3]) -> ([f32; 2], f32) {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        // yaw about z
        let x = p[0] * cy - p[1] * sy;
        let y = p[0] * sy + p[1] * cy;
        // tilt about the screen x axis: `up` mixes height and depth
        let up = p[2] * cp + y * sp;
        let depth = y * cp - p[2] * sp;
        (
            [
                self.center[0] + self.zoom * x,
                self.center[1] - self.zoom * up,
            ],
            depth,
        )
    }
}

/// How a grid is turned into a mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceOptions {
    /// Columns kept after block averaging (the throttle axis).
    pub max_cols: usize,
    /// Rows kept after block averaging (the frequency axis).
    pub max_rows: usize,
    /// Colour and height maximum; `<= 0` picks [`robust_max`] over the rows
    /// with `y >= min_y_for_scale`.
    pub max: f64,
    /// Rows below this `y` are ignored by the automatic maximum.
    pub min_y_for_scale: f64,
    /// Darken faces turned away from the light.
    pub shade: bool,
}

impl Default for SurfaceOptions {
    fn default() -> Self {
        SurfaceOptions {
            max_cols: 128,
            max_rows: 192,
            max: 0.0,
            min_y_for_scale: 0.0,
            shade: true,
        }
    }
}

/// Block-average `g` down to at most `max_cols × max_rows` cells; the
/// coordinates of a block are the mean of its members.
pub fn decimate(g: &Grid2D, max_cols: usize, max_rows: usize) -> Grid2D {
    let (nx, ny) = (g.x.len(), g.y.len());
    let bx = nx.div_ceil(max_cols.max(1)).max(1);
    let by = ny.div_ceil(max_rows.max(1)).max(1);
    if bx == 1 && by == 1 {
        return g.clone();
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let x: Vec<f64> = g.x.chunks(bx).map(mean).collect();
    let y: Vec<f64> = g.y.chunks(by).map(mean).collect();
    let mut z = Vec::with_capacity(x.len() * y.len());
    for row_block in (0..ny).step_by(by) {
        let rows = row_block..(row_block + by).min(ny);
        for col_block in (0..nx).step_by(bx) {
            let cols = col_block..(col_block + bx).min(nx);
            let mut sum = 0.0;
            let mut n = 0usize;
            for iy in rows.clone() {
                for v in &g.z[iy * nx + cols.start..iy * nx + cols.end] {
                    if v.is_finite() {
                        sum += v;
                        n += 1;
                    }
                }
            }
            z.push(if n == 0 { 0.0 } else { sum / n as f64 });
        }
    }
    Grid2D {
        x,
        y,
        z,
        x_label: g.x_label.clone(),
        y_label: g.y_label.clone(),
        z_label: g.z_label.clone(),
    }
}

/// Colour maximum of `g` for `opts` (explicit or robust).
pub fn colour_max(g: &Grid2D, opts: &SurfaceOptions) -> f64 {
    if opts.max > 0.0 {
        return opts.max;
    }
    let nx = g.x.len();
    let rows: Vec<f64> =
        g.y.iter()
            .enumerate()
            .filter(|(_, y)| **y >= opts.min_y_for_scale)
            .flat_map(|(iy, _)| g.z[iy * nx..(iy + 1) * nx].iter().copied())
            .collect();
    robust_max(if rows.is_empty() { &g.z } else { &rows })
}

/// Model-space point for data coordinates `(x, y, z)` on grid `g`.
pub fn model_point(g: &Grid2D, x: f64, y: f64, z: f64, max: f64, height: f32) -> [f32; 3] {
    let span = |v: &[f64]| -> (f64, f64) {
        let (a, b) = (
            v.first().copied().unwrap_or(0.0),
            v.last().copied().unwrap_or(1.0),
        );
        if (b - a).abs() < f64::EPSILON {
            (a - 0.5, a + 0.5)
        } else {
            (a, b)
        }
    };
    let (x0, x1) = span(&g.x);
    let (y0, y1) = span(&g.y);
    [
        ((x - x0) / (x1 - x0) - 0.5) as f32,
        ((y - y0) / (y1 - y0) - 0.5) as f32,
        ((z / max).clamp(0.0, 1.0) as f32) * height,
    ]
}

/// Surface height at data coordinates `(x, y)`: the nearest cell's value
/// (`None` outside the grid).
pub fn value_at(g: &Grid2D, x: f64, y: f64) -> Option<f64> {
    let nearest = |v: &[f64], t: f64| -> Option<usize> {
        if v.is_empty() {
            return None;
        }
        let (lo, hi) = (v[0].min(v[v.len() - 1]), v[0].max(v[v.len() - 1]));
        if t < lo || t > hi {
            return None;
        }
        v.iter()
            .enumerate()
            .min_by(|a, b| (a.1 - t).abs().total_cmp(&(b.1 - t).abs()))
            .map(|(i, _)| i)
    };
    let ix = nearest(&g.x, x)?;
    let iy = nearest(&g.y, y)?;
    Some(g.z[iy * g.x.len() + ix])
}

/// Projected, shaded mesh of a (decimated) grid.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SurfaceMesh {
    /// Screen position of every grid node, row-major like `Grid2D::z`.
    pub positions: Vec<[f32; 2]>,
    /// RGBA of every node.
    pub colors: Vec<[u8; 4]>,
    /// Triangle list, back to front, two triangles per cell.
    pub indices: Vec<u32>,
    /// Columns of the node grid.
    pub cols: usize,
    /// Rows of the node grid.
    pub rows: usize,
}

/// Build the mesh of `g` (already decimated) for `cam`; `max` is the colour
/// and height maximum.
pub fn build(g: &Grid2D, max: f64, cam: &Camera, shade: bool) -> SurfaceMesh {
    let (nx, ny) = (g.x.len(), g.y.len());
    let mut mesh = SurfaceMesh {
        cols: nx,
        rows: ny,
        ..Default::default()
    };
    if nx < 2 || ny < 2 {
        return mesh;
    }
    let max = if max > 0.0 { max } else { 1.0 };
    let heights: Vec<f32> =
        g.z.iter()
            .map(|z| ((z / max).clamp(0.0, 1.0) as f32) * cam.height)
            .collect();
    // light from the upper left, fixed in model space so orbiting shows the relief
    let light = {
        let l = [-0.4f32, -0.5, 0.75];
        let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        [l[0] / n, l[1] / n, l[2] / n]
    };
    let dx = 1.0 / (nx - 1) as f32;
    let dy = 1.0 / (ny - 1) as f32;
    let mut depths = Vec::with_capacity(nx * ny);
    mesh.positions.reserve(nx * ny);
    mesh.colors.reserve(nx * ny);
    for iy in 0..ny {
        for ix in 0..nx {
            let h = heights[iy * nx + ix];
            let p = [ix as f32 * dx - 0.5, iy as f32 * dy - 0.5, h];
            let (s, d) = cam.project(p);
            mesh.positions.push(s);
            depths.push(d);
            let mut c = viridis(g.z[iy * nx + ix] / max);
            if c[3] == 0 {
                c = [68, 1, 84, 255]; // the palette's floor, opaque: the surface has no holes
            }
            if shade {
                // normal from central differences
                let hx = heights[iy * nx + (ix + 1).min(nx - 1)]
                    - heights[iy * nx + ix.saturating_sub(1)];
                let hy = heights[(iy + 1).min(ny - 1) * nx + ix]
                    - heights[iy.saturating_sub(1) * nx + ix];
                let n = [-hx / (2.0 * dx), -hy / (2.0 * dy), 1.0];
                let len = (n[0] * n[0] + n[1] * n[1] + 1.0).sqrt();
                let dot = (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]) / len;
                let k = 0.55 + 0.45 * dot.max(0.0);
                for ch in &mut c[..3] {
                    *ch = (f32::from(*ch) * k).round().clamp(0.0, 255.0) as u8;
                }
            }
            mesh.colors.push(c);
        }
    }
    // painter's order: cells sorted far to near by their mean depth
    let mut cells: Vec<(f32, u32)> = Vec::with_capacity((nx - 1) * (ny - 1));
    for iy in 0..ny - 1 {
        for ix in 0..nx - 1 {
            let i = iy * nx + ix;
            let d = depths[i] + depths[i + 1] + depths[i + nx] + depths[i + nx + 1];
            cells.push((d, i as u32));
        }
    }
    cells.sort_by(|a, b| b.0.total_cmp(&a.0));
    let nx = nx as u32;
    mesh.indices.reserve(cells.len() * 6);
    for (_, i) in cells {
        mesh.indices
            .extend_from_slice(&[i, i + 1, i + nx, i + 1, i + nx + 1, i + nx]);
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(nx: usize, ny: usize) -> Grid2D {
        let x: Vec<f64> = (0..nx)
            .map(|i| i as f64 * 100.0 / (nx - 1) as f64)
            .collect();
        let y: Vec<f64> = (0..ny)
            .map(|i| i as f64 * 500.0 / (ny - 1) as f64)
            .collect();
        let z = (0..ny)
            .flat_map(|iy| (0..nx).map(move |ix| (ix + iy) as f64))
            .collect();
        Grid2D {
            x,
            y,
            z,
            x_label: "throttle (%)".into(),
            y_label: "frequency (Hz)".into(),
            z_label: "amplitude".into(),
        }
    }

    #[test]
    fn projection_is_consistent_with_the_view() {
        let cam = Camera {
            center: [100.0, 200.0],
            ..Camera::default()
        };
        // the origin lands on the centre whatever the angles
        let (s, _) = cam.project([0.0, 0.0, 0.0]);
        assert_eq!(s, [100.0, 200.0]);
        // height moves points up the screen (smaller y)
        let (base, _) = cam.project([0.2, 0.1, 0.0]);
        let (top, _) = cam.project([0.2, 0.1, 0.3]);
        assert!(top[1] < base[1]);
        assert_eq!(top[0], base[0]);
        // straight down: a pure 2D map, height does not move points
        let down = Camera {
            pitch: Camera::MAX_PITCH,
            ..cam
        };
        let (a, _) = down.project([0.2, 0.1, 0.0]);
        let (b, _) = down.project([0.2, 0.1, 0.5]);
        assert!((a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4);
        // farther points have larger depth: +y is away from the viewer at yaw 0
        let front = Camera { yaw: 0.0, ..cam };
        assert!(front.project([0.0, 0.5, 0.0]).1 > front.project([0.0, -0.5, 0.0]).1);
    }

    #[test]
    fn normalise_clamps() {
        let mut cam = Camera {
            yaw: 10.0,
            pitch: 3.0,
            zoom: 1.0,
            height: 9.0,
            ..Camera::default()
        };
        cam.normalise();
        assert!(cam.yaw.abs() <= std::f32::consts::PI);
        assert_eq!(cam.pitch, Camera::MAX_PITCH);
        assert_eq!(cam.zoom, 20.0);
        assert_eq!(cam.height, 2.0);
        cam.pitch = -1.0;
        cam.normalise();
        assert_eq!(cam.pitch, Camera::MIN_PITCH);
    }

    #[test]
    fn decimation_averages_blocks() {
        let g = grid(10, 20);
        let d = decimate(&g, 5, 4);
        assert_eq!((d.x.len(), d.y.len()), (5, 4));
        assert_eq!(d.z.len(), 20);
        // first block: columns 0..2, rows 0..5 → mean of (ix + iy) = 0.5 + 2 = 2.5
        assert!((d.z[0] - 2.5).abs() < 1e-9);
        assert!((d.x[0] - f64::midpoint(g.x[0], g.x[1])).abs() < 1e-9);
        // nothing to do: an identical copy
        assert_eq!(decimate(&g, 10, 20), g);
        assert_eq!(decimate(&g, 100, 100), g);
        // the odd remainder forms a smaller last block
        let d = decimate(&g, 4, 20);
        assert_eq!(d.x.len(), 4);
    }

    #[test]
    fn maximum_point_and_value_lookup() {
        let g = grid(11, 6);
        assert_eq!(
            colour_max(
                &g,
                &SurfaceOptions {
                    max: 3.0,
                    ..SurfaceOptions::default()
                }
            ),
            3.0
        );
        let auto = colour_max(&g, &SurfaceOptions::default());
        assert!(auto > 10.0 && auto <= 15.0, "{auto}");
        let p = model_point(&g, 0.0, 0.0, 0.0, 10.0, 0.5);
        assert_eq!(p, [-0.5, -0.5, 0.0]);
        let p = model_point(&g, 100.0, 500.0, 20.0, 10.0, 0.5);
        assert_eq!(p, [0.5, 0.5, 0.5]); // flattened at the colour maximum
        assert_eq!(value_at(&g, 0.0, 0.0), Some(0.0));
        assert_eq!(value_at(&g, 100.0, 500.0), Some(15.0));
        assert_eq!(value_at(&g, 52.0, 101.0), Some(5.0 + 1.0));
        assert_eq!(value_at(&g, -1.0, 0.0), None);
        assert_eq!(value_at(&g, 0.0, 501.0), None);
        // a degenerate axis still maps inside the unit square
        let mut flat = grid(3, 3);
        flat.x = vec![5.0, 5.0, 5.0];
        let p = model_point(&flat, 5.0, 0.0, 0.0, 1.0, 1.0);
        assert!((p[0]).abs() < 1e-6);
    }

    #[test]
    fn mesh_covers_the_grid_back_to_front() {
        let g = grid(10, 20);
        let cam = Camera::default();
        let m = build(&g, 19.0, &cam, true);
        assert_eq!((m.cols, m.rows), (10, 20));
        assert_eq!(m.positions.len(), 200);
        assert_eq!(m.colors.len(), 200);
        assert_eq!(m.indices.len(), 9 * 19 * 6);
        assert!(m.indices.iter().all(|i| (*i as usize) < 200));
        assert!(m.colors.iter().all(|c| c[3] == 255), "opaque");
        // far cells come first: the first cell's depth is at least the last's
        let depth = |i: u32| {
            let (ix, iy) = ((i % 10) as f32, (i / 10) as f32);
            cam.project([ix / 9.0 - 0.5, iy / 19.0 - 0.5, 0.0]).1
        };
        let first = m.indices[0];
        let last = m.indices[m.indices.len() - 6];
        assert!(depth(first) >= depth(last) - 0.2);
        // shading darkens or keeps colours, never brightens
        let flat = build(&g, 19.0, &cam, false);
        for (a, b) in m.colors.iter().zip(&flat.colors) {
            assert!(a[0] <= b[0] && a[1] <= b[1] && a[2] <= b[2]);
        }
        // the zero colour (transparent in the palette) is opaque on the surface
        let mut zero = grid(3, 3);
        zero.z = vec![0.0; 9];
        let m = build(&zero, 0.0, &cam, true);
        assert!(m.colors.iter().all(|c| c[3] == 255));
        // overlay points land on the mesh nodes they belong to
        let m = build(&g, 19.0, &cam, false);
        for (i, (ix, iy)) in [(0usize, 0usize), (9, 19), (4, 7)].into_iter().enumerate() {
            let node = iy * 10 + ix;
            let mp = model_point(&g, g.x[ix], g.y[iy], g.z[node], 19.0, cam.height);
            let (s, _) = cam.project(mp);
            assert!(
                (s[0] - m.positions[node][0]).abs() < 1e-3
                    && (s[1] - m.positions[node][1]).abs() < 1e-3,
                "{i}: {s:?} vs {:?}",
                m.positions[node]
            );
        }
        // too small to form a cell
        let tiny = decimate(&g, 1, 1);
        assert!(build(&tiny, 1.0, &cam, true).indices.is_empty());
    }
}
