// Included inside render::tests; no test-only public game API is needed.
fn fixture_triangle(x: [i16; 3], y: [i16; 3]) -> Triangle {
    let area = (x[1] as i32 - x[0] as i32) * (y[2] as i32 - y[0] as i32)
        - (y[1] as i32 - y[0] as i32) * (x[2] as i32 - x[0] as i32);
    Triangle {
        x,
        y,
        depth: 100,
        color: 7,
        reserved: if area > 0 { POSITIVE_AREA } else { 0 },
    }
}

#[test]
fn static_sky_reuse_requires_complete_clouds_and_excludes_all_occluder_bounds() {
    let mut renderer = Renderer::new();
    let add_clouds = |renderer: &mut Renderer, count: usize| {
        for _ in 0..count {
            renderer.triangle(
                Vec3 {
                    x: -1000,
                    y: 400,
                    z: 3000,
                },
                Vec3 {
                    x: 0,
                    y: 1500,
                    z: 3000,
                },
                Vec3 {
                    x: 1000,
                    y: 400,
                    z: 3000,
                },
                1,
                TWO_SIDED | crate::game::STATIC_SKY,
            );
        }
    };
    renderer.begin(6);
    add_clouds(&mut renderer, 8);
    assert_eq!(renderer.static_sky_bands(), 0);
    add_clouds(&mut renderer, 1);
    assert_eq!(renderer.triangle_count, 9);
    assert!(renderer.triangles[..9]
        .iter()
        .all(|triangle| triangle.reserved & crate::game::STATIC_SKY != 0));
    assert_eq!(renderer.static_sky_bands(), 0x01ff_ffff);
    renderer.dropped = 1;
    assert_eq!(renderer.static_sky_bands(), 0);
    renderer.dropped = 0;
    // A far-away, black face still blocks reuse, even if clouds hide its pixels.
    renderer.triangles[9] = fixture_triangle([20, 40, 30], [15, 16, 18]);
    renderer.triangles[9].depth = 31000;
    renderer.triangles[9].color = 0;
    renderer.triangle_count = 10;
    assert_eq!(renderer.static_sky_bands(), 0x01ff_ffff & !6);
    renderer.triangles[9].y = [-8191, 8191, 0];
    assert_eq!(renderer.static_sky_bands(), 0);
    renderer.begin(6);
    add_clouds(&mut renderer, 9);
    assert_eq!(
        renderer.static_sky_bands(),
        0x01ff_ffff,
        "discard stale occluders beyond current count"
    );
    drop(renderer.raster_pass().unwrap());
    assert_eq!(
        renderer.static_sky_bands(),
        0,
        "consumed bytes are not geometry"
    );
    renderer.begin(0);
    add_clouds(&mut renderer, 9);
    assert_eq!(renderer.static_sky_bands(), 0);
    renderer.begin(6);
    add_clouds(&mut renderer, 10);
    assert_eq!(renderer.static_sky_bands(), 0);
}

fn each_raster_fixture(mut visit: impl FnMut(Triangle)) {
    let authored = [
        ([0, 160, 0], [0, 0, 200]),
        ([160, 160, 0], [0, 200, 200]),
        ([-8191, 8191, 0], [-8191, 8191, 100]),
        ([-8191, 8191, 8191], [-8191, -8191, 8191]),
        ([-8191, 8191, -8191], [8191, -8191, -8191]),
        ([0, 159, 80], [-8191, 8191, 100]),
        ([0, 1, 0], [0, 200, 200]),
        ([159, 160, 160], [0, 0, 200]),
        ([20, 80, 130], [0, 8, 200]),
        ([20, 80, 130], [-7, 0, 8]),
        ([20, 80, 130], [191, 192, 200]),
        ([20, 80, 130], [192, 200, 201]),
        ([0, 160, 80], [0, 1, 0]),
        ([0, 160, 80], [199, 200, 200]),
        ([20, 80, 130], [-100, -50, -1]),
        ([20, 80, 130], [200, 201, 300]),
        ([81, 80, 80], [-8191, -8190, 8191]),
        ([-8191, 8191, 8190], [0, 1, 200]),
    ];
    for (x, y) in authored {
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            visit(fixture_triangle(order.map(|i| x[i]), order.map(|i| y[i])));
        }
    }
    let mut random = 0xc903_24abu32;
    for case in 0..1000 {
        let mut x = [0; 3];
        let mut y = [0; 3];
        for corner in 0..3 {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            x[corner] = if case & 1 == 0 {
                (random % 16383) as i16 - 8191
            } else {
                (random % 400) as i16 - 120
            };
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            y[corner] = if case & 1 == 0 {
                (random % 16383) as i16 - 8191
            } else {
                (random % 400) as i16 - 100
            };
        }
        visit(fixture_triangle(x, y));
    }
}

// Independent i64 rational oracle: intersect edges at exact scanline centers,
// sort fractions by cross products, and ceil(x-1/2). It shares no Q16 setup.
fn rational_bounds(triangle: &Triangle, row: i16) -> Option<(i64, i64)> {
    let mut fractions = [(0i64, 1i64); 3];
    let mut count = 0;
    for edge in 0..3 {
        let next = (edge + 1) % 3;
        let mut a = (triangle.x[edge] as i64, triangle.y[edge] as i64);
        let mut b = (triangle.x[next] as i64, triangle.y[next] as i64);
        if a.1 > b.1 {
            core::mem::swap(&mut a, &mut b);
        }
        if row as i64 >= a.1 && (row as i64) < b.1 {
            let dy = b.1 - a.1;
            fractions[count] = (
                2 * a.0 * dy + (b.0 - a.0) * (2 * (row as i64 - a.1) + 1),
                2 * dy,
            );
            count += 1;
        }
    }
    if count < 2 {
        return None;
    }
    fractions[..count].sort_by(|a, b| (a.0 * b.1).cmp(&(b.0 * a.1)));
    let round = |(n, d): (i64, i64)| (-(-(2 * n - d)).div_euclid(2 * d)).clamp(0, 160);
    Some((round(fractions[0]), round(fractions[count - 1])))
}

fn legacy_bounds(triangle: &Triangle, row: i16) -> Option<(i64, i64)> {
    let mut intersections = [0i64; 3];
    let mut count = 0;
    for edge in 0..3 {
        let next = (edge + 1) % 3;
        let mut a = (triangle.x[edge] as i64, triangle.y[edge] as i64);
        let mut b = (triangle.x[next] as i64, triangle.y[next] as i64);
        if a.1 > b.1 {
            core::mem::swap(&mut a, &mut b);
        }
        if row as i64 >= a.1 && (row as i64) < b.1 {
            let start = a.1.max((row / 8 * 8) as i64);
            let dx = b.0 - a.0;
            let dy = b.1 - a.1;
            let t = ((2 * (start - a.1) + 1) << 14) / dy;
            intersections[count] =
                (a.0 << 8) + ((dx * t) >> 7) + (row as i64 - start) * ((dx << 8) / dy);
            count += 1;
        }
    }
    if count < 2 {
        return None;
    }
    let round = |value: i64| ((value + 127) >> 8).clamp(0, 160);
    Some((
        round(*intersections[..count].iter().min().unwrap()),
        round(*intersections[..count].iter().max().unwrap()),
    ))
}

#[test]
fn q16_spans_stay_within_one_pixel_of_independent_oracle_and_legacy() {
    each_raster_fixture(|triangle| {
        let (initial, tail) = prepare_triangle(&triangle);
        for row in 0..200u8 {
            let Some(oracle) = rational_bounds(&triangle, row as i16) else {
                continue;
            };
            let legacy = legacy_bounds(&triangle, row as i16).unwrap();
            let mut head = initial;
            seek_row(&mut head, &tail, row);
            let mut endpoints = [head.long_x, head.short_x];
            endpoints.sort();
            let actual = endpoints.map(|x| ((x as i64 + 32767) >> 16).clamp(0, 160));
            for expected in [oracle, legacy] {
                assert!(
                    (actual[0] - expected.0).abs() <= 1 && (actual[1] - expected.1).abs() <= 1,
                    "x{:?} y{:?} row{row}: {actual:?} vs {expected:?}",
                    triangle.x,
                    triangle.y
                );
            }
        }
    });
}

#[test]
fn shared_edges_cover_tessellations_without_gaps_or_double_ownership() {
    for top in [0, 1, 7, 8, 9, 99] {
        for bottom in [101, 191, 192, 199, 200] {
            for diagonal in [false, true] {
                let mut a = Renderer::new();
                let mut b = Renderer::new();
                let (first, second) = if diagonal {
                    (
                        fixture_triangle([17, 143, 143], [top, top, bottom]),
                        fixture_triangle([17, 143, 17], [top, bottom, bottom]),
                    )
                } else {
                    (
                        fixture_triangle([17, 143, 17], [top, top, bottom]),
                        fixture_triangle([143, 143, 17], [top, bottom, bottom]),
                    )
                };
                a.triangles[0] = first;
                b.triangles[0] = second;
                a.triangle_count = 1;
                b.triangle_count = 1;
                let mut pa = a.raster_pass().unwrap();
                let mut pb = b.raster_pass().unwrap();
                for band in 0..25 {
                    let mut ap = [0; BAND_PIXELS];
                    let mut bp = [0; BAND_PIXELS];
                    pa.next_band(&mut ap, true);
                    pb.next_band(&mut bp, true);
                    for i in 0..BAND_PIXELS {
                        let x = i % 160;
                        let y = band * 8 + i / 160;
                        let expected =
                            (17..143).contains(&x) && (top as usize..bottom as usize).contains(&y);
                        assert_eq!(
                            (ap[i] != 0) as u8 + (bp[i] != 0) as u8,
                            expected as u8,
                            "pixel{x},{y}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "native kernel runner exports checked production edge records"]
fn export_native_raster_fixtures() {
    extern crate std;
    use std::fmt::Write;
    let path = std::env::var("HAVOC_RASTER_FIXTURES").expect("fixture output path");
    let mut output = std::string::String::from("static const struct Fixture fixtures[]={\n");
    let mut count = 0;
    each_raster_fixture(|triangle| {
        if count >= 256 {
            return;
        }
        let (h, t) = prepare_triangle(&triangle);
        writeln!(
            &mut output,
            "{{{{{},{},{},{}}},{{{},{},{},{},{},{}}}}},",
            h.long_x,
            h.long_step,
            h.short_x,
            h.short_step,
            t.lower_x,
            t.lower_step,
            t.top,
            t.middle,
            t.bottom,
            t.color_flags
        )
        .unwrap();
        count += 1;
    });
    output.push_str("};\n");
    std::fs::write(path, output).unwrap();
}

#[test]
fn persistent_raster_matches_arbitrary_band_reference_and_restarts() {
    let mut random = 0x0915_ba74u32;
    for count in [0, 1, 51, 256, 3] {
        let mut renderer = Renderer::new();
        for index in 0..count {
            let mut triangle = Triangle::EMPTY;
            for corner in 0..3 {
                random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                triangle.x[corner] = ((random >> 16) % 16383) as i16 - 8191;
                random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                triangle.y[corner] = ((random >> 16) % 16383) as i16 - 8191;
            }
            triangle.color = (index % 15 + 1) as u8;
            triangle.reserved = fixture_triangle(triangle.x, triangle.y).reserved;
            renderer.triangles[index] = triangle;
        }
        renderer.triangle_count = count as u16;
        let mut expected = [[0; BAND_PIXELS]; 25];
        for band in (0..25).rev() {
            renderer.render_band(band, &mut expected[band as usize]);
        }
        {
            let mut pass = renderer.raster_pass().unwrap();
            let mut actual = [0; BAND_PIXELS];
            for band in 0..25 {
                let paint = band % 3 != 1;
                actual.fill(0xa5);
                assert_eq!(pass.next_band(&mut actual, paint), Some(band));
                if paint {
                    assert_eq!(actual, expected[band as usize], "count{count} band{band}");
                } else {
                    assert_eq!(actual, [0xa5; BAND_PIXELS]);
                }
            }
            actual.fill(0xa5);
            assert_eq!(pass.next_band(&mut actual, true), None);
            assert_eq!(actual, [0xa5; BAND_PIXELS]);
        }
        assert!(renderer.raster_pass().is_none());
        renderer.begin(6);
        let mut pass = renderer.raster_pass().unwrap();
        let mut pixels = [0; BAND_PIXELS];
        for band in 0..25 {
            assert_eq!(pass.next_band(&mut pixels, true), Some(band));
            assert_eq!(pixels, [6; BAND_PIXELS]);
        }
    }
}

#[test]
fn near_intersections_are_independent_of_winding() {
    for a in [
        Vec3 {
            x: -32768,
            y: 32767,
            z: -32768,
        },
        Vec3 {
            x: -17,
            y: 31,
            z: 31,
        },
    ] {
        for b in [
            Vec3 {
                x: 32767,
                y: -32768,
                z: 32767,
            },
            Vec3 {
                x: 71,
                y: -19,
                z: 33,
            },
        ] {
            assert_eq!(near_intersection(a, b), near_intersection(b, a));
        }
    }
}
