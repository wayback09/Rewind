//! Coordinate conventions — version-independent, deterministic.
//!
//! World coordinates follow Minecraft canonical:
//! - X increases east, Z increases south, Y vertical.
//! - chunk (cx,cz) contains world x in [cx*16, cx*16+15], z in [cz*16, cz*16+15].
//! - section_y = floor_div(world_y, 16)  (Euclidean, so negative Y works)
//! - y_base = section_y * 16
//! - local (lx,ly,lz) in 0..15 where lx = world_x mod 16 etc.
//! - block index inside a section: idx = (ly * 16 + lz) * 16 + lx

/// Convert world X/Z to chunk coordinates (floor-div by 16).
pub fn world_to_chunk(world_x: i32, world_z: i32) -> (i32, i32) {
    (world_x.div_euclid(16), world_z.div_euclid(16))
}

/// Chunk origin in world coordinates (min corner).
pub fn chunk_origin(chunk_x: i32, chunk_z: i32) -> (i32, i32) {
    (chunk_x * 16, chunk_z * 16)
}

/// World Y to section Y (floor div).
pub fn world_y_to_section_y(world_y: i32) -> i32 {
    world_y.div_euclid(16)
}

/// Section Y to its world Y base (minimum Y in section).
pub fn section_y_to_y_base(section_y: i32) -> i32 {
    section_y * 16
}

/// World coordinate to local 0..15 inside its chunk/section.
pub fn world_to_local(world_coord: i32) -> i32 {
    world_coord.rem_euclid(16)
}

/// Local coordinates + section Y to world position.
pub fn local_to_world(
    chunk_x: i32,
    chunk_z: i32,
    section_y: i32,
    lx: i32,
    ly: i32,
    lz: i32,
) -> (i32, i32, i32) {
    let wx = chunk_x * 16 + lx;
    let wy = section_y * 16 + ly;
    let wz = chunk_z * 16 + lz;
    (wx, wy, wz)
}

/// Section-local block index: idx = (ly*16+lz)*16+lx, 0..4095.
pub fn local_to_index(lx: usize, ly: usize, lz: usize) -> usize {
    debug_assert!(lx < 16 && ly < 16 && lz < 16);
    (ly * 16 + lz) * 16 + lx
}

/// Inverse of local_to_index.
pub fn index_to_local(idx: usize) -> (usize, usize, usize) {
    debug_assert!(idx < 4096);
    let lx = idx % 16;
    let lz = (idx / 16) % 16;
    let ly = idx / 256;
    (lx, ly, lz)
}

/// World position to chunk/section/local tuple.
pub fn world_to_chunk_section_local(
    world_x: i32,
    world_y: i32,
    world_z: i32,
) -> ((i32, i32), i32, (i32, i32, i32)) {
    let (cx, cz) = world_to_chunk(world_x, world_z);
    let sy = world_y_to_section_y(world_y);
    let lx = world_to_local(world_x);
    let ly = world_to_local(world_y);
    let lz = world_to_local(world_z);
    ((cx, cz), sy, (lx, ly, lz))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_origin_round_trip() {
        for (wx, wz) in [
            (0, 0),
            (15, 15),
            (16, 16),
            (-1, -1),
            (-16, -16),
            (-17, -17),
            (100, -200),
        ] {
            let (cx, cz) = world_to_chunk(wx, wz);
            let (ox, oz) = chunk_origin(cx, cz);
            assert!(wx >= ox && wx < ox + 16, "wx {} ox {} cx {}", wx, ox, cx);
            assert!(wz >= oz && wz < oz + 16);
            assert_eq!(world_to_local(wx), wx - ox);
            assert_eq!(world_to_local(wz), wz - oz);
        }
    }

    #[test]
    fn section_y_negative() {
        assert_eq!(world_y_to_section_y(-64), -4);
        assert_eq!(world_y_to_section_y(-65), -5);
        assert_eq!(world_y_to_section_y(-1), -1);
        assert_eq!(world_y_to_section_y(0), 0);
        assert_eq!(world_y_to_section_y(15), 0);
        assert_eq!(world_y_to_section_y(16), 1);
        assert_eq!(world_y_to_section_y(319), 19);
        assert_eq!(world_y_to_section_y(320), 20);
        assert_eq!(section_y_to_y_base(-4), -64);
        assert_eq!(section_y_to_y_base(0), 0);
        assert_eq!(section_y_to_y_base(19), 304);
    }

    #[test]
    fn index_round_trip_corners_and_center() {
        let corners = [
            (0, 0, 0),
            (15, 0, 0),
            (0, 15, 0),
            (0, 0, 15),
            (15, 15, 15),
            (7, 7, 7),
            (8, 8, 8),
        ];
        for (lx, ly, lz) in corners {
            let idx = local_to_index(lx, ly, lz);
            let (rx, ry, rz) = index_to_local(idx);
            assert_eq!((lx, ly, lz), (rx, ry, rz));
        }
        // linear scan
        for idx in 0..4096 {
            let (lx, ly, lz) = index_to_local(idx);
            assert_eq!(local_to_index(lx, ly, lz), idx);
        }
    }

    #[test]
    fn negative_coordinates_chunk_boundaries() {
        // chunk -1 spans -16..-1
        let (cx, cz) = world_to_chunk(-1, -1);
        assert_eq!((cx, cz), (-1, -1));
        let (ox, oz) = chunk_origin(cx, cz);
        assert_eq!((ox, oz), (-16, -16));
        assert_eq!(world_to_local(-1), 15);
        assert_eq!(world_to_local(-16), 0);
        assert_eq!(world_to_local(-17), 15);
        assert_eq!(world_to_chunk(-17, -17), (-2, -2));
        // section boundary at y=-64
        assert_eq!(world_y_to_section_y(-64), -4);
        assert_eq!(world_to_local(-64), 0);
        assert_eq!(world_y_to_section_y(-49), -4);
        assert_eq!(world_to_local(-49), 15);
        assert_eq!(world_y_to_section_y(-48), -3);
    }

    #[test]
    fn world_to_chunk_section_local_negative_y() {
        let ((cx, cz), sy, (lx, ly, lz)) = world_to_chunk_section_local(-7, -64, 1);
        assert_eq!((cx, cz), (-1, 0));
        assert_eq!(sy, -4);
        assert_eq!((lx, ly, lz), (9, 0, 1));
        let ((cx2, cz2), sy2, (lx2, ly2, lz2)) = world_to_chunk_section_local(-112, 62, 16);
        assert_eq!((cx2, cz2), (-7, 1));
        assert_eq!(sy2, 3);
        assert_eq!((lx2, ly2, lz2), (0, 14, 0));
    }

    #[test]
    fn local_to_world_round_trip() {
        let (wx, wy, wz) = local_to_world(-7, 1, 3, 5, 2, 9);
        assert_eq!((wx, wy, wz), (-107, 50, 25));
        let ((cx, cz), sy, (lx, ly, lz)) = world_to_chunk_section_local(wx, wy, wz);
        assert_eq!((cx, cz), (-7, 1));
        assert_eq!(sy, 3);
        assert_eq!((lx, ly, lz), (5, 2, 9));
    }
}
