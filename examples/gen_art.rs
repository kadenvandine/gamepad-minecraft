// gamepad-minecraft - a controller-first Minecraft Java launcher
//
// Copyright (C) 2026 Ken VanDine
// SPDX-License-Identifier: GPL-3.0-or-later

//! Generates the launcher's bundled artwork into `data/art/`:
//!
//! - `hero.png` - the Play tab's key art: an original crimson-forest
//!   Nether scene, rendered by the small voxel ray tracer below
//!   (procedural 16x16 block textures, flood-filled block light, sun
//!   shafts through the canopy, volumetric fog, bloom).
//! - `logo.png` - the blocky stone "MINECRAFT" wordmark with "JAVA
//!   EDITION" underneath.
//! - `grass_block.png` - the isometric grass block icon shown beside the
//!   selected installation.
//!
//! Everything is drawn from code - no Mojang artwork, fonts or textures
//! are used - and the output is deterministic, so re-running this only
//! changes the PNGs when this file changes. The launcher just bundles
//! the results with `include_bytes!`; nothing here runs at startup.
//!
//!     cargo run --release --example gen_art            # full quality
//!     cargo run --release --example gen_art -- preview # quick, low-res

use std::ops::{Add, Mul, Neg, Sub};
use std::path::Path;

use gamepad_minecraft::pixelart;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;

// ─── Math ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct V3 {
    x: f32,
    y: f32,
    z: f32,
}

const fn v3(x: f32, y: f32, z: f32) -> V3 {
    V3 { x, y, z }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f32> for V3 {
    type Output = V3;
    fn mul(self, s: f32) -> V3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
}
impl Mul<V3> for V3 {
    type Output = V3;
    fn mul(self, o: V3) -> V3 {
        v3(self.x * o.x, self.y * o.y, self.z * o.z)
    }
}
impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        v3(-self.x, -self.y, -self.z)
    }
}

impl V3 {
    fn dot(self, o: V3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    fn cross(self, o: V3) -> V3 {
        v3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    fn norm(self) -> V3 {
        self * (1.0 / self.dot(self).sqrt())
    }
    fn get(self, i: usize) -> f32 {
        [self.x, self.y, self.z][i]
    }
    fn max_c(self) -> f32 {
        self.x.max(self.y).max(self.z)
    }
}

fn mix(a: V3, b: V3, t: f32) -> V3 {
    a * (1.0 - t) + b * t
}

/// sRGB hex -> linear-light colour.
fn rgb(hex: u32) -> V3 {
    let c = |s: u32| ((hex >> s) & 0xFF) as f32 / 255.0;
    let lin = |v: f32| v.powf(2.2);
    v3(lin(c(16)), lin(c(8)), lin(c(0)))
}

fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

fn h3(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    hash(
        (x as u32).wrapping_mul(73_856_093)
            ^ (y as u32).wrapping_mul(19_349_663)
            ^ (z as u32).wrapping_mul(83_492_791)
            ^ seed.wrapping_mul(2_654_435_761),
    )
}

fn r3(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    h3(x, y, z, seed) as f32 / u32::MAX as f32
}

/// Smooth 2D value noise in [0, 1].
fn noise2(x: f32, z: f32, seed: u32) -> f32 {
    let (xi, zi) = (x.floor() as i32, z.floor() as i32);
    let (fx, fz) = (x - x.floor(), z - z.floor());
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sz) = (s(fx), s(fz));
    let a = r3(xi, 0, zi, seed);
    let b = r3(xi + 1, 0, zi, seed);
    let c = r3(xi, 0, zi + 1, seed);
    let d = r3(xi + 1, 0, zi + 1, seed);
    (a * (1.0 - sx) + b * sx) * (1.0 - sz) + (c * (1.0 - sx) + d * sx) * sz
}

fn fbm2(x: f32, z: f32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut freq = 1.0;
    for o in 0..4 {
        sum += noise2(x * freq, z * freq, seed + o) * amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / 0.9375
}

// ─── Blocks & textures ──────────────────────────────────────────────

const AIR: u8 = 0;
const NETHERRACK: u8 = 1;
const NYLIUM: u8 = 2;
const STEM: u8 = 3;
const WART: u8 = 4;
const SHROOM: u8 = 5;
const LAVA: u8 = 6;
const BLACKSTONE: u8 = 7;
const MAGMA: u8 = 8;
// Everything from here on is a "cross" plant: two diagonal textured
// planes inside the cell rather than a full cube.
const VINES: u8 = 9;
const ROOTS: u8 = 10;
const FUNGUS: u8 = 11;
const FIRE: u8 = 12;

fn is_cross(b: u8) -> bool {
    b >= VINES
}
fn is_opaque(b: u8) -> bool {
    b != AIR && !is_cross(b)
}

#[derive(Clone, Copy, PartialEq)]
enum Face {
    Top,
    Bottom,
    Side,
}

struct Texel {
    col: V3,
    emit: V3,
    alpha: bool,
}

fn solid(col: V3) -> Texel {
    Texel { col, emit: V3::default(), alpha: true }
}
fn glowing(col: V3, k: f32) -> Texel {
    Texel { col, emit: col * k, alpha: true }
}
const CLEAR: Texel = Texel { col: v3(0.0, 0.0, 0.0), emit: v3(0.0, 0.0, 0.0), alpha: false };

fn netherrack(tx: i32, ty: i32, s: u32) -> Texel {
    // Clumpy: 2x2-ish clusters plus single-pixel specks.
    let clump = r3(tx / 2, ty / 2, (tx + ty) % 3, s);
    let speck = r3(tx, ty, 7, s);
    let mut col = mix(rgb(0x5a2828), rgb(0x8c3d39), clump);
    if speck < 0.12 {
        col = rgb(0x3e191b);
    } else if speck > 0.94 {
        col = rgb(0xa9524a);
    }
    solid(col)
}

fn texel(b: u8, face: Face, tx: i32, ty: i32, cell: (i32, i32, i32)) -> Texel {
    let s = h3(cell.0, cell.1, cell.2, b as u32);
    let r = r3(tx, ty, 1, s);
    let r2 = r3(tx, ty, 2, s);
    match b {
        NETHERRACK => netherrack(tx, ty, s),
        NYLIUM => {
            let fringe = 2 + (h3(tx, 0, 0, s) % 4) as i32;
            if face == Face::Side && ty >= fringe {
                return netherrack(tx, ty, s);
            }
            let mut col = mix(rgb(0x7a1116), rgb(0xa8201e), r);
            if r2 < 0.16 {
                col = rgb(0x560c14);
            } else if r2 > 0.92 {
                col = rgb(0xc8402e);
            }
            solid(col)
        }
        STEM => {
            if face != Face::Side {
                let d = (tx as f32 - 7.5).abs().max((ty as f32 - 7.5).abs());
                if d < 6.0 {
                    let ring = (d as i32) % 2 == 0;
                    let col = if ring { rgb(0x8a3a3a) } else { rgb(0x6a2a30) };
                    return solid(col * (0.9 + r * 0.2));
                }
            }
            // Vertical bark stripes, with the odd glowing fleck crimson
            // stems are known for.
            let stripe = r3(tx, ty / 4, 3, s);
            const BARK: [u32; 4] = [0x3a0f1e, 0x4e1628, 0x5e1c30, 0x742638];
            let col = rgb(BARK[(stripe * 3.99) as usize]);
            solid(col * (0.9 + r * 0.2))
        }
        WART => {
            let mut col = mix(rgb(0x660a0c), rgb(0x941616), r);
            if r2 > 0.86 {
                col = rgb(0xb42a22);
            } else if r2 < 0.1 {
                col = rgb(0x46060a);
            }
            solid(col)
        }
        SHROOM => {
            let grid = (tx + (ty / 4) * 2) % 4 == 0 || (ty + (tx / 4)) % 4 == 0;
            let col = if grid {
                rgb(0xc8622a)
            } else if r > 0.7 {
                rgb(0xffd27a)
            } else {
                rgb(0xf3a04a)
            };
            glowing(col, 2.4)
        }
        LAVA => {
            let n = noise2(tx as f32 / 3.0 + cell.0 as f32 * 5.3, ty as f32 / 3.0 + cell.2 as f32 * 5.3, 11);
            glowing(mix(rgb(0xb8380a), rgb(0xffb43a), n * n), 3.2)
        }
        BLACKSTONE => {
            let mut col = mix(rgb(0x1c181e), rgb(0x3a3238), r3(tx / 2, ty, 4, s));
            if r2 > 0.95 {
                col = rgb(0x4e444c);
            }
            solid(col)
        }
        MAGMA => {
            let crack = r3(tx / 3, ty / 3, 5, s) < 0.45 && (tx % 3 == 0 || ty % 3 == 1);
            if crack {
                glowing(mix(rgb(0xff6a10), rgb(0xffb040), r), 2.6)
            } else {
                solid(mix(rgb(0x30100a), rgb(0x52200e), r))
            }
        }
        VINES => {
            let center = 7.5 + ((ty as f32) * 0.9 + s as f32).sin() * 2.2;
            if (tx as f32 - center).abs() < 1.4 + r2 * 0.9 {
                solid(mix(rgb(0x8a1212), rgb(0xc42a22), r))
            } else {
                CLEAR
            }
        }
        ROOTS => {
            for (i, x0) in [3.0f32, 7.5, 12.0].iter().enumerate() {
                let height = 8 + (h3(i as i32, 0, 0, s) % 8) as i32;
                let lean = (r3(i as i32, 1, 0, s) - 0.5) * 0.5;
                let x = x0 + (15 - ty) as f32 * lean;
                if ty > 15 - height && (tx as f32 - x).abs() < 1.0 {
                    return solid(mix(rgb(0x8a1426), rgb(0xd0384a), r));
                }
            }
            CLEAR
        }
        FUNGUS => {
            if (10..16).contains(&ty) && (7..9).contains(&tx) {
                return solid(rgb(0x6a2a1a));
            }
            if (3..10).contains(&ty) && (tx as f32 - 7.5).abs() < (ty - 2) as f32 * 0.95 {
                return if r2 > 0.82 {
                    glowing(rgb(0xffc070), 1.0)
                } else {
                    solid(mix(rgb(0x9a1a18), rgb(0xc42a20), r))
                };
            }
            CLEAR
        }
        FIRE => {
            let height = 5.0 + 10.0 * noise2(tx as f32 * 0.55, s as f32 * 0.001, 13);
            if (15 - ty) as f32 <= height {
                let t = (15 - ty) as f32 / 15.0;
                glowing(mix(rgb(0xffd070), rgb(0xff4a0a), t), 2.2)
            } else {
                CLEAR
            }
        }
        _ => solid(rgb(0xff00ff)),
    }
}

/// Coloured block light each emitter seeds into the light volume.
fn emission(b: u8) -> V3 {
    match b {
        SHROOM => v3(2.2, 1.15, 0.45),
        LAVA => v3(2.6, 0.85, 0.18),
        FIRE => v3(3.2, 1.5, 0.5),
        MAGMA => v3(0.9, 0.3, 0.08),
        _ => V3::default(),
    }
}

// ─── World ──────────────────────────────────────────────────────────

const W: i32 = 72;
const H: i32 = 44;
const D: i32 = 112;

struct World {
    cells: Vec<u8>,
    light: Vec<V3>,
    sun: Vec<f32>,
    ground: Vec<i32>,
}

fn idx(x: i32, y: i32, z: i32) -> usize {
    ((z * H + y) * W + x) as usize
}
fn in_bounds(x: i32, y: i32, z: i32) -> bool {
    (0..W).contains(&x) && (0..H).contains(&y) && (0..D).contains(&z)
}

impl World {
    fn get(&self, x: i32, y: i32, z: i32) -> u8 {
        if in_bounds(x, y, z) {
            self.cells[idx(x, y, z)]
        } else {
            AIR
        }
    }
    fn set(&mut self, x: i32, y: i32, z: i32, b: u8) {
        if in_bounds(x, y, z) {
            self.cells[idx(x, y, z)] = b;
        }
    }
    fn ground_at(&self, x: i32, z: i32) -> i32 {
        self.ground[(z.clamp(0, D - 1) * W + x.clamp(0, W - 1)) as usize]
    }
}

/// Where the hero character stands, and the campfire beside them.
const PLAYER_POS: V3 = v3(36.3, 2.0, 5.2);
const FIRE_CELL: (i32, i32, i32) = (32, 2, 8);
/// Direction *towards* the light: high above and behind the scene, so
/// shafts slant down towards the camera.
fn sun_dir() -> V3 {
    v3(-0.18, 1.0, 0.62).norm()
}

fn build_world() -> World {
    let mut w = World {
        cells: vec![AIR; (W * H * D) as usize],
        light: Vec::new(),
        sun: Vec::new(),
        ground: vec![0; (W * D) as usize],
    };

    // Ground: rolling netherrack under crimson nylium, a flat clearing
    // for the hero, rising blackstone walls framing both sides, and a
    // lava lake glowing in the distance.
    for z in 0..D {
        for x in 0..W {
            let (fx, fz) = (x as f32, z as f32);
            let mut g = 1.0 + fbm2(fx * 0.09, fz * 0.09, 1) * 2.4;
            let clearing = ((fx - 36.0).abs() / 16.0).max((fz - 10.0).abs() / 16.0);
            if clearing < 1.0 {
                g = mix(v3(1.0, 0.0, 0.0), v3(g, 0.0, 0.0), clearing.powi(2)).x;
            }
            let edge = ((14.0 - fx).max(fx - 57.0)).max(0.0);
            g += edge * edge * 0.12 * (0.6 + fbm2(fx * 0.2, fz * 0.2, 2) * 0.8);
            let lake = ((fx - 36.0) / 20.0).powi(2) + ((fz - 92.0) / 13.0).powi(2);
            let mut lava = false;
            if lake < 1.0 {
                g = 0.0;
                lava = true;
            }
            let g = (g as i32).min(H - 4);
            w.ground[(z * W + x) as usize] = g;
            for y in 0..=g {
                let rocky = edge > 1.5 && fbm2(fx * 0.3, fy_hash(y, fz), 3) > 0.45;
                w.set(x, y, z, if rocky { BLACKSTONE } else { NETHERRACK });
            }
            if lava {
                w.set(x, 0, z, LAVA);
            } else if edge < 1.5 && fbm2(fx * 0.15, fz * 0.15, 4) > 0.32 {
                w.set(x, g, z, NYLIUM);
            }
            if !lava && r3(x, g, z, 21) < 0.012 && (z > 22 || (fx - 35.0).abs() > 8.0) {
                w.set(x, g, z, MAGMA);
            }
        }
    }

    // Canopy: a ceiling of wart blocks with a few ragged openings for
    // the light shafts.
    for z in 0..D {
        for x in 0..W {
            let (fx, fz) = (x as f32, z as f32);
            let lo = 23 + (fbm2(fx * 0.08, fz * 0.08, 5) * 6.0) as i32;
            let mut hole = fbm2(fx * 0.11, fz * 0.11, 6) > 0.63;
            for (hx, hz, r) in [(34.0, 33.0, 4.5), (22.0, 24.0, 3.0), (49.0, 40.0, 3.5), (40.0, 58.0, 4.0)] {
                if ((fx - hx).powi(2) + (fz - hz).powi(2)).sqrt() < r + fbm2(fx * 0.5, fz * 0.5, 7) * 2.0 {
                    hole = true;
                }
            }
            if hole || z > 100 {
                continue;
            }
            for y in lo..34 {
                w.set(x, y, z, WART);
            }
            if r3(x, lo, z, 8) < 0.025 {
                w.set(x, lo, z, SHROOM);
            }
        }
    }

    // Huge crimson fungi: stems up to the canopy, each crowned with a
    // hanging curtain of wart blocks studded with shroomlights.
    let trees: [(i32, i32, i32); 17] = [
        (24, 9, 1),
        (51, 6, 1),
        (11, 22, 2),
        (27, 30, 1),
        (47, 25, 2),
        (63, 19, 2),
        (36, 46, 1),
        (21, 48, 2),
        (53, 51, 1),
        (7, 40, 1),
        (66, 42, 2),
        (29, 64, 2),
        (45, 68, 1),
        (15, 70, 1),
        (58, 73, 2),
        (37, 80, 1),
        (24, 84, 1),
    ];
    for (i, &(tx, tz, r)) in trees.iter().enumerate() {
        let seed = 100 + i as u32;
        let base = w.ground_at(tx, tz);
        let top = 23 + (fbm2(tx as f32 * 0.08, tz as f32 * 0.08, 5) * 6.0) as i32;
        let rr = r as f32 + 0.45;
        for y in base..top + 1 {
            for dz in -r..=r {
                for dx in -r..=r {
                    if ((dx * dx + dz * dz) as f32).sqrt() <= rr {
                        w.set(tx + dx, y, tz + dz, STEM);
                    }
                }
            }
        }
        let cap_r = (r + 4 + (hash(seed) % 3) as i32) as f32;
        let cap_drop = 5 + (hash(seed + 1) % 4) as i32;
        let ri = cap_r as i32 + 1;
        for dz in -ri..=ri {
            for dx in -ri..=ri {
                let d = ((dx * dx + dz * dz) as f32).sqrt();
                if d > cap_r + 0.4 {
                    continue;
                }
                let (x, z) = (tx + dx, tz + dz);
                let edge = d > cap_r - 1.2;
                let len = if edge {
                    cap_drop - (r3(x, 0, z, seed) * 3.0) as i32
                } else {
                    1 + (r3(x, 1, z, seed) * 2.0) as i32
                };
                for y in (top - len)..=top {
                    let inner = d > cap_r - 2.4 && d <= cap_r - 1.2;
                    let b = if inner && r3(x, y, z, seed + 2) < 0.12 { SHROOM } else { WART };
                    w.set(x, y, z, b);
                }
                if edge && r3(x, 2, z, seed) < 0.12 {
                    w.set(x, top - len - 1, z, SHROOM);
                }
            }
        }
    }

    // Weeping vines under every overhang, plants on the ground.
    for z in 0..D {
        for x in 0..W {
            for y in (2..H - 1).rev() {
                if w.get(x, y, z) == AIR && matches!(w.get(x, y + 1, z), WART | SHROOM) {
                    if r3(x, y, z, 30) < 0.16 {
                        let len = 1 + (r3(x, y, z, 31) * 8.0) as i32;
                        for k in 0..len {
                            if w.get(x, y - k, z) != AIR || y - k <= w.ground_at(x, z) + 2 {
                                break;
                            }
                            w.set(x, y - k, z, VINES);
                        }
                    }
                    break;
                }
            }
            let g = w.ground_at(x, z);
            if w.get(x, g, z) == NYLIUM && w.get(x, g + 1, z) == AIR {
                let near_hero = (x - 36).abs() < 5 && (3..10).contains(&z);
                let p = r3(x, g, z, 32);
                if !near_hero && p < 0.06 {
                    w.set(x, g + 1, z, ROOTS);
                } else if !near_hero && p < 0.08 {
                    w.set(x, g + 1, z, FUNGUS);
                }
            }
        }
    }
    // Extra foreground plants framing the bottom corners.
    for z in 0..7 {
        for x in (16..30).chain(44..58) {
            let g = w.ground_at(x, z);
            if w.get(x, g + 1, z) == AIR && r3(x, z, 0, 33) < 0.18 {
                w.set(x, g + 1, z, if r3(x, z, 1, 33) < 0.8 { ROOTS } else { FUNGUS });
            }
        }
    }

    // The campfire: a fire on a netherrack hearth ringed by blackstone.
    let (fx, fy, fz) = FIRE_CELL;
    w.set(fx, fy - 1, fz, NETHERRACK);
    w.set(fx, fy, fz, FIRE);
    for (dx, dz) in [(-1, 0), (1, 0), (0, 1), (0, -1)] {
        if r3(dx, 0, dz, 40) < 0.7 {
            w.set(fx + dx, fy - 1, fz + dz, BLACKSTONE);
        }
    }

    w.light = compute_block_light(&w);
    w.sun = compute_sun_visibility(&w);
    w
}

fn fy_hash(y: i32, fz: f32) -> f32 {
    y as f32 * 0.37 + fz * 0.3
}

/// Minecraft-style flood fill of coloured light out of every emitter,
/// decaying per block and blocked by opaque cells - then sampled
/// trilinearly so the result is smooth rather than stepped.
fn compute_block_light(w: &World) -> Vec<V3> {
    let mut light = vec![V3::default(); w.cells.len()];
    for (i, &b) in w.cells.iter().enumerate() {
        light[i] = emission(b);
    }
    const DECAY: f32 = 0.82;
    for _ in 0..16 {
        let prev = light.clone();
        for z in 0..D {
            for y in 0..H {
                for x in 0..W {
                    let b = w.get(x, y, z);
                    if is_opaque(b) {
                        continue;
                    }
                    let mut best = prev[idx(x, y, z)];
                    for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
                        let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                        if !in_bounds(nx, ny, nz) {
                            continue;
                        }
                        let n = prev[idx(nx, ny, nz)] * DECAY;
                        best = v3(best.x.max(n.x), best.y.max(n.y), best.z.max(n.z));
                    }
                    light[idx(x, y, z)] = best;
                }
            }
        }
    }
    light
}

fn compute_sun_visibility(w: &World) -> Vec<f32> {
    let dir = sun_dir();
    let mut sun = vec![0.0; w.cells.len()];
    for z in 0..D {
        for y in 0..H {
            for x in 0..W {
                if is_opaque(w.get(x, y, z)) {
                    continue;
                }
                let o = v3(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                if trace_world(w, o, dir, 200.0, true).is_none() {
                    sun[idx(x, y, z)] = 1.0;
                }
            }
        }
    }
    sun
}

/// Trilinear sample of a per-cell field at cell centres.
fn sample_grid<T: Copy + Default + Add<Output = T> + Mul<f32, Output = T>>(field: &[T], p: V3) -> T {
    let q = p - v3(0.5, 0.5, 0.5);
    let (x0, y0, z0) = (q.x.floor() as i32, q.y.floor() as i32, q.z.floor() as i32);
    let (fx, fy, fz) = (q.x - x0 as f32, q.y - y0 as f32, q.z - z0 as f32);
    let mut acc = T::default();
    for dz in 0..2 {
        for dy in 0..2 {
            for dx in 0..2 {
                let (x, y, z) = (x0 + dx, y0 + dy, z0 + dz);
                let wgt = if dx == 1 { fx } else { 1.0 - fx }
                    * if dy == 1 { fy } else { 1.0 - fy }
                    * if dz == 1 { fz } else { 1.0 - fz };
                let v = if in_bounds(x, y, z) { field[idx(x, y, z)] } else { T::default() };
                acc = acc + v * wgt;
            }
        }
    }
    acc
}

// ─── Ray tracing ────────────────────────────────────────────────────

struct Hit {
    t: f32,
    n: V3,
    col: V3,
    emit: V3,
    /// Plants/entities skip the cube-face ambient occlusion pass.
    flat: bool,
}

fn ray_box(o: V3, d: V3, lo: V3, hi: V3) -> Option<(f32, f32, usize)> {
    let mut t0 = f32::NEG_INFINITY;
    let mut t1 = f32::INFINITY;
    let mut axis = 0;
    for i in 0..3 {
        let inv = 1.0 / d.get(i);
        let mut a = (lo.get(i) - o.get(i)) * inv;
        let mut b = (hi.get(i) - o.get(i)) * inv;
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        if a > t0 {
            t0 = a;
            axis = i;
        }
        t1 = t1.min(b);
    }
    (t0 <= t1 && t1 > 0.0).then_some((t0, t1, axis))
}

/// Amanatides-Woo voxel walk. `shadow` skips fire (it shouldn't cast
/// shadows) and the texture lookups nobody will read.
fn trace_world(w: &World, o: V3, d: V3, tmax: f32, shadow: bool) -> Option<Hit> {
    let (t_enter, t_exit, enter_axis) =
        ray_box(o, d, v3(0.0, 0.0, 0.0), v3(W as f32, H as f32, D as f32))?;
    let mut t = t_enter.max(0.0);
    let p = o + d * (t + 1e-4);
    let mut cell = [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32];
    let step = [d.x.signum() as i32, d.y.signum() as i32, d.z.signum() as i32];
    let mut t_max = [0.0f32; 3];
    let mut t_delta = [0.0f32; 3];
    for i in 0..3 {
        let di = d.get(i);
        t_delta[i] = if di != 0.0 { (1.0 / di).abs() } else { f32::INFINITY };
        let boundary = if di > 0.0 { cell[i] as f32 + 1.0 } else { cell[i] as f32 };
        t_max[i] = if di != 0.0 { (boundary - o.get(i)) / di } else { f32::INFINITY };
    }
    let mut axis = if t_enter > 0.0 { enter_axis } else { 1 };
    let limit = t_exit.min(tmax);
    while t < limit {
        let b = w.get(cell[0], cell[1], cell[2]);
        let t_next = t_max[0].min(t_max[1]).min(t_max[2]);
        if is_opaque(b) {
            if shadow {
                return Some(Hit { t, n: V3::default(), col: V3::default(), emit: V3::default(), flat: false });
            }
            let mut n = V3::default();
            let sgn = -(step[axis] as f32);
            match axis {
                0 => n.x = sgn,
                1 => n.y = sgn,
                _ => n.z = sgn,
            }
            let hp = o + d * t;
            let l = hp - v3(cell[0] as f32, cell[1] as f32, cell[2] as f32);
            let tc = |v: f32| ((v * 16.0) as i32).clamp(0, 15);
            let (face, tx, ty) = match axis {
                1 => (if n.y > 0.0 { Face::Top } else { Face::Bottom }, tc(l.x), tc(l.z)),
                0 => (Face::Side, tc(l.z), tc(1.0 - l.y)),
                _ => (Face::Side, tc(l.x), tc(1.0 - l.y)),
            };
            let tex = texel(b, face, tx, ty, (cell[0], cell[1], cell[2]));
            return Some(Hit { t, n, col: tex.col, emit: tex.emit, flat: false });
        }
        if is_cross(b) && !(shadow && b == FIRE) {
            if let Some(hit) = cross_hit(o, d, cell, b, t, t_next) {
                return Some(hit);
            }
        }
        axis = if t_max[0] < t_max[1] {
            if t_max[0] < t_max[2] { 0 } else { 2 }
        } else if t_max[1] < t_max[2] {
            1
        } else {
            2
        };
        t = t_max[axis];
        cell[axis] += step[axis];
        t_max[axis] += t_delta[axis];
    }
    None
}

/// Intersects the two diagonal planes of a plant cell, nearest first.
fn cross_hit(o: V3, d: V3, cell: [i32; 3], b: u8, t0: f32, t1: f32) -> Option<Hit> {
    let c = v3(cell[0] as f32 + 0.5, 0.0, cell[2] as f32 + 0.5);
    let mut best: Option<Hit> = None;
    for sign in [1.0f32, -1.0] {
        // Plane: (x - cx) - sign * (z - cz) = 0
        let denom = d.x - sign * d.z;
        if denom.abs() < 1e-6 {
            continue;
        }
        let t = -((o.x - c.x) - sign * (o.z - c.z)) / denom;
        if t < t0 - 1e-4 || t > t1 + 1e-4 || best.as_ref().is_some_and(|h| h.t < t) {
            continue;
        }
        let p = o + d * t;
        let along = (p.x - c.x + 0.5).clamp(0.0, 0.999);
        let ly = p.y - cell[1] as f32;
        let tx = (along * 16.0) as i32;
        let ty = ((1.0 - ly) * 16.0).clamp(0.0, 15.0) as i32;
        let tex = texel(b, Face::Side, tx, ty, (cell[0], cell[1], cell[2]));
        if tex.alpha {
            let n = v3(1.0, 0.0, -sign).norm();
            let n = if n.dot(d) > 0.0 { -n } else { n };
            best = Some(Hit { t, n: (n + v3(0.0, 0.6, 0.0)).norm(), col: tex.col, emit: tex.emit, flat: true });
        }
    }
    best
}

// ─── The hero character ─────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Part {
    Head,
    Body,
    Arm,
    Leg,
}

struct Obb {
    c: V3,
    ax: [V3; 3],
    half: V3,
    part: Part,
}

fn rot_y(v: V3, a: f32) -> V3 {
    v3(v.x * a.cos() + v.z * a.sin(), v.y, -v.x * a.sin() + v.z * a.cos())
}
fn rot_x(v: V3, a: f32) -> V3 {
    v3(v.x, v.y * a.cos() - v.z * a.sin(), v.y * a.sin() + v.z * a.cos())
}

/// A blocky adventurer mid-stride, facing (mostly) towards the camera.
/// Sizes are in skin pixels (1/16 block), like the game's own models.
fn build_character() -> Vec<Obb> {
    let yaw = 0.3;
    let px = 1.0 / 16.0;
    let mut parts = Vec::new();
    // (part, centre, size, pivot, swing about X, extra yaw)
    let defs: [(Part, V3, V3, V3, f32, f32); 6] = [
        (Part::Head, v3(0.0, 28.0, 0.0), v3(8.0, 8.0, 8.0), v3(0.0, 24.0, 0.0), -0.08, -0.25),
        (Part::Body, v3(0.0, 18.0, 0.0), v3(8.0, 12.0, 4.0), v3(0.0, 18.0, 0.0), 0.0, 0.0),
        (Part::Arm, v3(-6.0, 18.0, 0.0), v3(4.0, 12.0, 4.0), v3(-6.0, 22.0, 0.0), -0.75, 0.0),
        (Part::Arm, v3(6.0, 18.0, 0.0), v3(4.0, 12.0, 4.0), v3(6.0, 22.0, 0.0), 0.55, 0.0),
        (Part::Leg, v3(-2.0, 6.0, 0.0), v3(4.0, 12.0, 4.0), v3(-2.0, 12.0, 0.0), 0.5, 0.0),
        (Part::Leg, v3(2.0, 6.0, 0.0), v3(4.0, 12.0, 4.0), v3(2.0, 12.0, 0.0), -0.45, 0.0),
    ];
    for (part, centre, size, pivot, swing, extra_yaw) in defs {
        let limb = |v: V3| rot_y(rot_x(v, swing), extra_yaw);
        let model_c = pivot + limb(centre - pivot);
        let world_c = PLAYER_POS + rot_y(model_c, yaw) * px;
        let tf = |v: V3| rot_y(limb(v), yaw);
        let ax = [tf(v3(1.0, 0.0, 0.0)), tf(v3(0.0, 1.0, 0.0)), tf(v3(0.0, 0.0, 1.0))];
        parts.push(Obb { c: world_c, ax, half: size * (0.5 * px), part });
    }
    parts
}

fn character_texel(part: Part, axis: usize, front: bool, u: i32, v: i32, size: [i32; 3]) -> V3 {
    let skin = rgb(0xd59a6a);
    let hair = rgb(0x4a2a14);
    let shirt = rgb(0x2c8c88);
    let jitter = 0.9 + r3(u, v, axis as i32, part as u32 + if front { 9 } else { 0 }) * 0.2;
    let col = match part {
        Part::Head => match (axis, front) {
            (1, top) => if top { hair } else { skin },
            (2, true) => {
                if v < 2 || (v == 2 && (u == 0 || u == 7)) {
                    hair
                } else if v == 4 && (u == 1 || u == 6) {
                    rgb(0xf0f0f0)
                } else if v == 4 && (u == 2 || u == 5) {
                    rgb(0x2a6a4a)
                } else if v == 6 && (3..5).contains(&u) {
                    rgb(0xa66a46)
                } else {
                    skin
                }
            }
            (2, false) => if v < 7 { hair } else { skin },
            _ => if v < 3 || (u >= 4 && v < 6) { hair } else { skin },
        },
        Part::Body => {
            if v >= size[1] - 2 {
                rgb(0x5a3a20)
            } else if axis == 2 && front && v < 1 && (3..5).contains(&u) {
                skin
            } else if (u + v) % 5 == 0 {
                rgb(0x237370)
            } else {
                shirt
            }
        }
        Part::Arm => if v < 5 { shirt } else { skin },
        Part::Leg => if v < 9 { rgb(0x2f3a6a) } else { rgb(0x4a3020) },
    };
    col * jitter
}

fn trace_character(parts: &[Obb], o: V3, d: V3, tmax: f32) -> Option<Hit> {
    let mut best: Option<Hit> = None;
    for p in parts {
        let rel = o - p.c;
        let lo_ = v3(rel.dot(p.ax[0]), rel.dot(p.ax[1]), rel.dot(p.ax[2]));
        let ld = v3(d.dot(p.ax[0]), d.dot(p.ax[1]), d.dot(p.ax[2]));
        let Some((t, _, axis)) = ray_box(lo_, ld, -p.half, p.half) else { continue };
        if t <= 0.0 || t > tmax || best.as_ref().is_some_and(|b| b.t < t) {
            continue;
        }
        let lp = lo_ + ld * t;
        let sign = -ld.get(axis).signum();
        let n = p.ax[axis] * sign;
        let size = [
            (p.half.x * 32.0).round() as i32,
            (p.half.y * 32.0).round() as i32,
            (p.half.z * 32.0).round() as i32,
        ];
        let pxc = |i: usize| (((lp.get(i) + p.half.get(i)) * 16.0) as i32).clamp(0, size[i] - 1);
        // The model faces local -z. For the Y axis, `front` instead
        // means "the top face".
        let front = if axis == 1 { sign > 0.0 } else { sign < 0.0 };
        let (u, v) = match axis {
            0 => (pxc(2), size[1] - 1 - pxc(1)),
            1 => (pxc(0), pxc(2)),
            _ => (pxc(0), size[1] - 1 - pxc(1)),
        };
        let col = character_texel(p.part, axis, front, u, v, size);
        best = Some(Hit { t, n, col, emit: V3::default(), flat: true });
    }
    best
}

// ─── Shading ────────────────────────────────────────────────────────

struct Scene {
    world: World,
    character: Vec<Obb>,
}

fn occluded(s: &Scene, p: V3, d: V3) -> bool {
    trace_world(&s.world, p, d, 200.0, true).is_some() || trace_character(&s.character, p, d, 200.0).is_some()
}

fn ambient_occlusion(w: &World, p: V3, n: V3) -> f32 {
    let q = p + n * 0.5;
    let (t1, t2) = if n.y.abs() > 0.5 {
        (v3(1.0, 0.0, 0.0), v3(0.0, 0.0, 1.0))
    } else {
        (v3(0.0, 1.0, 0.0), n.cross(v3(0.0, 1.0, 0.0)))
    };
    let mut occ = 0.0;
    for (a, b) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (0.7, 0.7), (-0.7, 0.7), (0.7, -0.7), (-0.7, -0.7)] {
        let s = q + t1 * (a * 0.6) + t2 * (b * 0.6);
        if is_opaque(w.get(s.x.floor() as i32, s.y.floor() as i32, s.z.floor() as i32)) {
            occ += 1.0;
        }
    }
    1.0 - occ * 0.075
}

const SUN_COL: V3 = v3(2.8, 1.65, 0.7);
const AMBIENT: V3 = v3(0.11, 0.035, 0.03);
const MAX_DIST: f32 = 150.0;

fn background(d: V3) -> V3 {
    // Past the end of the cavern: hot haze, brightest straight ahead.
    let ahead = d.z.max(0.0).powf(4.0);
    let up = d.y.max(0.0);
    v3(1.6, 0.6, 0.22) * (0.4 + ahead) + v3(2.0, 1.3, 0.7) * up * up
}

fn shade_ray(s: &Scene, o: V3, d: V3, jitter: f32) -> V3 {
    let hw = trace_world(&s.world, o, d, MAX_DIST, false);
    let t_world = hw.as_ref().map_or(MAX_DIST, |h| h.t);
    let hc = trace_character(&s.character, o, d, t_world);
    let is_char = hc.is_some();
    let hit = hc.or(hw);

    let (surface, t_end) = match &hit {
        None => (background(d), MAX_DIST),
        Some(h) => {
            let p = o + d * h.t;
            let lit_p = p + h.n * 0.02;
            let sd = sun_dir();
            let ndl = h.n.dot(sd).max(0.0);
            let direct = if ndl > 0.0 && !occluded(s, lit_p, sd) { SUN_COL * ndl } else { V3::default() };
            let block = sample_grid(&s.world.light, p + h.n * 0.5);
            let ao = if h.flat { 1.0 } else { ambient_occlusion(&s.world, p, h.n) };
            let mut light = direct + block * ao + AMBIENT * ((0.75 + 0.25 * h.n.y) * ao);
            if is_char {
                // A soft key from camera-left so the hero reads clearly.
                let key = v3(-0.6, 0.5, -0.65).norm();
                light = light + v3(1.0, 0.55, 0.32) * (h.n.dot(key).max(0.0) * 0.9) + v3(0.1, 0.05, 0.05);
            }
            (h.col * light + h.emit, h.t)
        }
    };

    // Volumetric fog: warm haze, sun shafts where the canopy lets light
    // through, and glow around every emitter.
    let sd = sun_dir();
    let cos = d.dot(sd);
    let g = 0.55;
    let phase = (1.0 - g * g) / (1.0 + g * g - 2.0 * g * cos).powf(1.5) * 0.25;
    let steps = ((t_end / 0.7) as usize).clamp(4, 200);
    let dt = t_end / steps as f32;
    let mut trans = 1.0;
    let mut scatter = V3::default();
    for i in 0..steps {
        let p = o + d * ((i as f32 + jitter) * dt);
        let density = 0.010 + 0.022 * (-(p.y - 1.0).max(0.0) / 7.0).exp();
        let sun = sample_grid(&s.world.sun, p);
        let glow = sample_grid(&s.world.light, p);
        let inscatter = SUN_COL * (sun * phase * 3.2) + glow * 0.3 + v3(0.15, 0.045, 0.018);
        scatter = scatter + inscatter * (trans * density * dt);
        trans *= (-density * dt).exp();
    }
    surface * trans + scatter
}

// ─── Image pipeline ─────────────────────────────────────────────────

struct Hdr {
    w: usize,
    h: usize,
    px: Vec<V3>,
}

impl Hdr {
    fn at(&self, x: usize, y: usize) -> V3 {
        self.px[y * self.w + x]
    }
}

fn render_hero(width: usize, height: usize, ss: usize) -> Hdr {
    let scene = Scene { world: build_world(), character: build_character() };
    let eye = v3(35.8, 3.5, -1.5);
    let pitch: f32 = 0.06;
    let fwd = v3(0.0, pitch.sin(), pitch.cos()).norm();
    let right = fwd.cross(v3(0.0, 1.0, 0.0)).norm() * -1.0;
    let up = right.cross(fwd) * -1.0;
    let tan_h = (78.0f32.to_radians() / 2.0).tan();
    let tan_v = tan_h * height as f32 / width as f32;

    let mut px = vec![V3::default(); width * height];
    let threads = std::thread::available_parallelism().map_or(8, |n| n.get());
    let rows_per = height.div_ceil(threads);
    std::thread::scope(|sc| {
        for (chunk_i, chunk) in px.chunks_mut(rows_per * width).enumerate() {
            let scene = &scene;
            sc.spawn(move || {
                for (i, out) in chunk.iter_mut().enumerate() {
                    let x = i % width;
                    let y = chunk_i * rows_per + i / width;
                    let mut acc = V3::default();
                    for sy in 0..ss {
                        for sx in 0..ss {
                            let u = ((x as f32 + (sx as f32 + 0.5) / ss as f32) / width as f32) * 2.0 - 1.0;
                            let v = ((y as f32 + (sy as f32 + 0.5) / ss as f32) / height as f32) * 2.0 - 1.0;
                            let d = (fwd + right * (u * tan_h) + up * (-v * tan_v)).norm();
                            let jitter = r3(x as i32, y as i32, (sy * ss + sx) as i32, 77);
                            acc = acc + shade_ray(scene, eye, d, jitter);
                        }
                    }
                    *out = acc * (1.0 / (ss * ss) as f32);
                }
            });
        }
    });
    Hdr { w: width, h: height, px }
}

/// Floating embers, drawn into the HDR buffer so bloom picks them up.
fn add_embers(img: &mut Hdr) {
    let scale = img.w as f32 / 1920.0;
    for i in 0..140 {
        let rx = r3(i, 0, 0, 501);
        let ry = r3(i, 1, 0, 501);
        // Bias towards the middle band and lower half, like drifting
        // sparks rising off the forest floor.
        let x = (0.5 + (rx - 0.5) * (0.6 + 0.4 * r3(i, 2, 0, 501))) * img.w as f32;
        let y = (0.3 + ry * 0.55) * img.h as f32;
        let size = ((3.0 + r3(i, 3, 0, 501) * 5.0) * scale).max(2.0) as i32;
        let heat = 0.8 + r3(i, 4, 0, 501) * 1.6;
        let col = mix(v3(1.0, 0.22, 0.04), v3(1.0, 0.55, 0.15), r3(i, 5, 0, 501)) * heat;
        for dy in 0..size {
            for dx in 0..size {
                let (px, py) = (x as i32 + dx, y as i32 + dy);
                if px >= 0 && py >= 0 && (px as usize) < img.w && (py as usize) < img.h {
                    let k = py as usize * img.w + px as usize;
                    img.px[k] = img.px[k] + col;
                }
            }
        }
    }
}

fn blur(src: &[V3], w: usize, h: usize, radius: i32) -> Vec<V3> {
    let sigma = radius as f32 / 2.5;
    let kernel: Vec<f32> = (-radius..=radius).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let sum: f32 = kernel.iter().sum();
    let pass = |src: &[V3], horizontal: bool| -> Vec<V3> {
        let mut out = vec![V3::default(); w * h];
        for y in 0..h {
            for x in 0..w {
                let mut acc = V3::default();
                for (k, wgt) in kernel.iter().enumerate() {
                    let o = k as i32 - radius;
                    let (sx, sy) = if horizontal {
                        ((x as i32 + o).clamp(0, w as i32 - 1) as usize, y)
                    } else {
                        (x, (y as i32 + o).clamp(0, h as i32 - 1) as usize)
                    };
                    acc = acc + src[sy * w + sx] * *wgt;
                }
                out[y * w + x] = acc * (1.0 / sum);
            }
        }
        out
    };
    pass(&pass(src, true), false)
}

fn aces(x: f32) -> f32 {
    ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0)
}

fn finish_hero(img: &Hdr) -> Vec<u8> {
    // Bloom from a quarter-resolution bright pass.
    let (bw, bh) = (img.w / 4, img.h / 4);
    let mut bright = vec![V3::default(); bw * bh];
    for y in 0..bh {
        for x in 0..bw {
            let mut acc = V3::default();
            for dy in 0..4 {
                for dx in 0..4 {
                    let c = img.at(x * 4 + dx, y * 4 + dy);
                    let excess = (c.max_c() - 0.9).max(0.0);
                    acc = acc + c * (excess / c.max_c().max(1e-4));
                }
            }
            bright[y * bw + x] = acc * (1.0 / 16.0);
        }
    }
    let wide = blur(&bright, bw, bh, (bw / 40).max(2) as i32);
    let tight = blur(&bright, bw, bh, (bw / 160).max(1) as i32);

    let mut out = Vec::with_capacity(img.w * img.h * 3);
    for y in 0..img.h {
        for x in 0..img.w {
            let (bx, by) = ((x / 4).min(bw - 1), (y / 4).min(bh - 1));
            let mut c = img.at(x, y) + wide[by * bw + bx] * 0.5 + tight[by * bw + bx] * 0.35;
            let (u, v) = (x as f32 / img.w as f32 - 0.5, y as f32 / img.h as f32 - 0.5);
            let vignette = 1.0 - (u * u * 1.1 + v * v * 1.3).min(1.0) * 0.75;
            c = c * (1.1 * vignette);
            let dither = (r3(x as i32, y as i32, 0, 909) - 0.5) / 255.0;
            for ch in [c.x, c.y, c.z] {
                let srgb = aces(ch).powf(1.0 / 2.2) + dither;
                out.push((srgb * 255.0).round().clamp(0.0, 255.0) as u8);
            }
        }
    }
    out
}

// ─── Logo ───────────────────────────────────────────────────────────

/// Chunky two-cell-stroke letters for the wordmark (original glyphs,
/// in the spirit of - not traced from - the game's own logo).
fn logo_glyph(c: char) -> &'static [&'static str; 8] {
    match c {
        'M' => &["##.....##", "###...###", "####.####", "##.###.##", "##..#..##", "##.....##", "##.....##", "##.....##"],
        'I' => &["######", "..##..", "..##..", "..##..", "..##..", "..##..", "..##..", "######"],
        'N' => &["##....##", "###...##", "####..##", "##.##.##", "##..####", "##...###", "##....##", "##....##"],
        'E' => &["#######", "##.....", "##.....", "######.", "##.....", "##.....", "##.....", "#######"],
        'C' => &[".######", "##....#", "##.....", "##.....", "##.....", "##.....", "##....#", ".######"],
        'R' => &["#######.", "##....##", "##....##", "#######.", "##.##...", "##..##..", "##...##.", "##....##"],
        'A' => &["..####..", ".##..##.", "##....##", "##....##", "########", "##....##", "##....##", "##....##"],
        'F' => &["#######", "##.....", "##.....", "######.", "##.....", "##.....", "##.....", "##....."],
        'T' => &["########", "...##...", "...##...", "...##...", "...##...", "...##...", "...##...", "...##..."],
        _ => &["", "", "", "", "", "", "", ""],
    }
}

struct Mask {
    w: i32,
    h: i32,
    on: Vec<bool>,
}

impl Mask {
    fn new(w: i32, h: i32) -> Self {
        Self { w, h, on: vec![false; (w * h) as usize] }
    }
    fn get(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h && self.on[(y * self.w + x) as usize]
    }
    fn set_rect(&mut self, x: i32, y: i32, s: i32) {
        for py in y..y + s {
            for px in x..x + s {
                if px >= 0 && py >= 0 && px < self.w && py < self.h {
                    self.on[(py * self.w + px) as usize] = true;
                }
            }
        }
    }
    fn dilate(&self, r: i32) -> Mask {
        let mut out = Mask::new(self.w, self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                if self.get(x, y) {
                    out.set_rect(x - r, y - r, 2 * r + 1);
                }
            }
        }
        out
    }
    fn shifted_union(&self, dx: i32, dy_max: i32) -> Mask {
        let mut out = Mask::new(self.w, self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                if (0..=dy_max).any(|dy| self.get(x - dx * dy / dy_max.max(1), y - dy)) {
                    out.on[(y * self.w + x) as usize] = true;
                }
            }
        }
        out
    }
}

/// Paints one line of blocky lettering: textured stone faces, a darker
/// extrusion below, and a thick black outline around both.
#[allow(clippy::too_many_arguments)]
fn paint_lettering(
    img: &mut pixelart::Image,
    cells: &[(i32, i32)],
    x0: i32,
    y0: i32,
    cell: i32,
    depth: i32,
    outline: i32,
    face_lo: u32,
    face_hi: u32,
    side: u32,
    seed: u32,
) {
    let mut face = Mask::new(img.width as i32, img.height as i32);
    for &(cx, cy) in cells {
        face.set_rect(x0 + cx * cell, y0 + cy * cell, cell);
    }
    let body = face.shifted_union(0, depth);
    let ring = body.dilate(outline);
    let texel = (cell / 4).max(1);
    let to_rgb = |hex: u32, k: f32| {
        let c = |s: u32| (((hex >> s) & 0xFF) as f32 * k).clamp(0.0, 255.0) as u8;
        [c(16), c(8), c(0), 255]
    };
    for y in 0..img.height as i32 {
        for x in 0..img.width as i32 {
            let color = if face.get(x, y) {
                let r = r3(x / texel, y / texel, 0, seed);
                // Bevel: lit top edge, shaded bottom edge of each stroke.
                let top_edge = !face.get(x, y - texel);
                let bottom_edge = !face.get(x, y + texel);
                let base = if r > 0.5 { face_hi } else { face_lo };
                let k = if top_edge { 1.18 } else if bottom_edge { 0.78 } else { 0.92 + r * 0.16 };
                to_rgb(base, k)
            } else if body.get(x, y) {
                let r = r3(x / texel, y / texel, 1, seed);
                to_rgb(side, 0.85 + r * 0.3)
            } else if ring.get(x, y) {
                [0x12, 0x10, 0x12, 255]
            } else {
                continue;
            };
            let i = ((y as u32 * img.width + x as u32) * 4) as usize;
            img.rgba[i..i + 4].copy_from_slice(&color);
        }
    }
}

/// Rendered at the size the launcher displays it (~600px wide):
/// `gtk::Picture` draws at the image's natural size.
fn render_logo() -> pixelart::Image {
    let cell = 8;
    let word = "MINECRAFT";
    let mut cells = Vec::new();
    let mut pen = 0;
    for ch in word.chars() {
        let g = logo_glyph(ch);
        for (y, row) in g.iter().enumerate() {
            for (x, b) in row.bytes().enumerate() {
                if b == b'#' {
                    cells.push((pen + x as i32, y as i32));
                }
            }
        }
        pen += g[0].len() as i32 + 1;
    }
    let word_w = (pen - 1) * cell;
    let margin = 6;
    let depth = cell + cell / 2;

    let ed_cell = 5;
    let edition = "JAVA EDITION";
    let ed_w = pixelart::text_width_cells(edition) as i32 * ed_cell;
    let ed_y = margin + 8 * cell + depth - ed_cell * 2;
    let height = ed_y + 7 * ed_cell + ed_cell + margin + 2;
    let width = word_w + margin * 2;
    let mut img = pixelart::Image::new(width as u32, height as u32);

    paint_lettering(&mut img, &cells, margin, margin, cell, depth, 3, 0xa8a4a0, 0xc4c0bb, 0x585452, 1);

    let mut ed_cells = Vec::new();
    pixelart::for_each_text_cell(edition, |x, y| ed_cells.push((x as i32, y as i32)));
    paint_lettering(
        &mut img,
        &ed_cells,
        (width - ed_w) / 2,
        ed_y,
        ed_cell,
        ed_cell,
        2,
        0xe8e4dc,
        0xf6f2ea,
        0x6c6660,
        2,
    );
    img
}

// ─── Grass block icon ───────────────────────────────────────────────

fn render_grass_block(n: i32) -> pixelart::Image {
    let hgt = (n as f32 * 1.12) as i32;
    let (w, h) = (2 * n, n + hgt);
    let mut img = pixelart::Image::new(w as u32, h as u32);
    let grass = |tx: i32, ty: i32| {
        let r = r3(tx, ty, 0, 600);
        if r > 0.85 {
            0x7cc53e
        } else if r < 0.2 {
            0x4a8a28
        } else {
            0x5d9c34
        }
    };
    let dirt = |tx: i32, ty: i32| {
        let r = r3(tx, ty, 1, 600);
        if r > 0.8 {
            0x9a6e48
        } else if r < 0.25 {
            0x6b4a2c
        } else {
            0x86603c
        }
    };
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let nf = n as f32;
            let ap = ((fx - nf) / nf + 2.0 * fy / nf) / 2.0;
            let bp = (2.0 * fy / nf - (fx - nf) / nf) / 2.0;
            let (hex, k) = if (0.0..1.0).contains(&ap) && (0.0..1.0).contains(&bp) {
                (grass((ap * 16.0) as i32, (bp * 16.0) as i32), 1.0)
            } else {
                let left = fx < nf;
                let a = if left { fx / nf } else { (fx - nf) / nf };
                let top = if left { nf / 2.0 + a * nf / 2.0 } else { nf - a * nf / 2.0 };
                let hv = (fy - top) / hgt as f32;
                if !(0.0..1.0).contains(&hv) {
                    continue;
                }
                let (tx, ty) = ((a * 16.0) as i32 + if left { 0 } else { 16 }, (hv * 16.0) as i32);
                let fringe = 3 + (h3(tx, 0, 0, 601) % 3) as i32;
                let hex = if ty < fringe { grass(tx, ty) } else { dirt(tx, ty) };
                (hex, if left { 0.62 } else { 0.8 })
            };
            let c = |s: u32| (((hex >> s) & 0xFF) as f32 * k) as u8;
            let i = ((y * w + x) * 4) as usize;
            img.rgba[i..i + 4].copy_from_slice(&[c(16), c(8), c(0), 255]);
        }
    }
    img
}

// ─── Output ─────────────────────────────────────────────────────────

fn save_png(path: &Path, w: u32, h: u32, data: Vec<u8>, has_alpha: bool) {
    let (format, bpp) = if has_alpha {
        (gdk::MemoryFormat::R8g8b8a8, 4)
    } else {
        (gdk::MemoryFormat::R8g8b8, 3)
    };
    let tex = gdk::MemoryTexture::new(w as i32, h as i32, format, &glib::Bytes::from_owned(data), (w * bpp) as usize);
    tex.save_to_png(path).expect("failed to write PNG");
    println!("wrote {}", path.display());
}

fn main() {
    let preview = std::env::args().any(|a| a == "preview");
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/art");
    std::fs::create_dir_all(&out_dir).unwrap();

    let logo = render_logo();
    save_png(&out_dir.join("logo.png"), logo.width, logo.height, logo.rgba, true);
    let grass = render_grass_block(48);
    save_png(&out_dir.join("grass_block.png"), grass.width, grass.height, grass.rgba, true);

    let (w, h, ss) = if preview { (960, 540, 1) } else { (1920, 1080, 3) };
    let started = std::time::Instant::now();
    let mut hdr = render_hero(w, h, ss);
    add_embers(&mut hdr);
    let rgb_bytes = finish_hero(&hdr);
    let name = if preview { "hero-preview.png" } else { "hero.png" };
    let path = if preview {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join(name)
    } else {
        out_dir.join(name)
    };
    save_png(&path, w as u32, h as u32, rgb_bytes, false);
    println!("hero rendered in {:.1}s", started.elapsed().as_secs_f32());
}
