// Appended inside a copy of the actual render module by test-raster-kernel.py.
// Keeping the reference and private layouts together avoids a public test API.
pub fn write_native_fixtures(path: &std::path::Path) {
    let mut data = std::vec::Vec::new();
    let mut state = 0x9173_2b45u32;
    for case in 0..256 {
        let mut triangle = Triangle::EMPTY;
        for i in 0..3 {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            triangle.x[i] = match case % 4 {
                0 => (state % 16_383) as i16 - 8191,
                1 => (state % 160) as i16,
                _ => (state % 1600) as i16 - 700,
            };
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            triangle.y[i] = match case % 4 {
                0 => (state % 16_383) as i16 - 8191,
                1 => (state % 200) as i16,
                2 => (state % 1600) as i16 - 700,
                _ => case as i16 / 2 + (state % 3) as i16,
            };
        }
        triangle.color = 1 + (case % 15) as u8;
        let slopes = triangle_slopes(&triangle);
        for band in 0..25i16 {
            let band_y = band * 8;
            let first = triangle.y[0].min(triangle.y[1]).min(triangle.y[2]).max(band_y);
            let end = triangle.y[0].max(triangle.y[1]).max(triangle.y[2]).min(band_y + 8);
            let mut rows = RasterRows { edges: [Edge::EMPTY; 3], first_y: 0, end_y: 0, band_y: band_y as u8, color: triangle.color };
            if first < end {
                rows.first_y = first as u8;
                rows.end_y = end as u8;
                for i in 0..3 {
                    let j = (i + 1) % 3;
                    let (ax, ay, bx, by) = if triangle.y[i] < triangle.y[j] {
                        (triangle.x[i], triangle.y[i], triangle.x[j], triangle.y[j])
                    } else {
                        (triangle.x[j], triangle.y[j], triangle.x[i], triangle.y[i])
                    };
                    if ay == by || by <= first || ay >= end { continue; }
                    let start = first.max(ay);
                    let dx = bx as i32 - ax as i32;
                    let dy = by as i32 - ay as i32;
                    let numerator = ((start as i32 - ay as i32) * 2 + 1) << 14;
                    let fraction = crate::math::div_u32_u16(numerator as u32, dy as u16) as i32;
                    rows.edges[i] = Edge { y0: ay, y1: by, x: ((ax as i32) << 8) + ((dx * fraction) >> 7), step: slopes[i] };
                }
            }
            for edge in rows.edges {
                data.extend_from_slice(&edge.y0.to_le_bytes());
                data.extend_from_slice(&edge.y1.to_le_bytes());
                data.extend_from_slice(&edge.x.to_le_bytes());
                data.extend_from_slice(&edge.step.to_le_bytes());
            }
            data.extend_from_slice(&[rows.first_y, rows.end_y, rows.band_y, rows.color]);
            let mut expected = [0; BAND_PIXELS];
            let mut independent = [0; BAND_PIXELS];
            raster_rows_reference(&mut rows, &mut expected);
            raster_triangle(&triangle, band_y, &mut independent, None);
            assert_eq!(expected, independent, "fixture edge setup differs from reference");
            for row in 0..8 {
                let pixels = &expected[row * WIDTH..row * WIDTH + WIDTH];
                let first = pixels.iter().position(|&p| p != 0).unwrap_or(0);
                let end = pixels.iter().rposition(|&p| p != 0).map_or(0, |x| x + 1);
                for (x, &pixel) in pixels.iter().enumerate() {
                    assert_eq!(pixel, if x >= first && x < end { triangle.color } else { 0 });
                }
                data.extend_from_slice(&[first as u8, end as u8]);
            }
        }
    }
    std::fs::write(path, data).unwrap();
}
