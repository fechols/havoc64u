use havoc::reticle::{screen_position, vic_position, COLOR, IMAGE};

#[test]
fn exactly_one_native_pixel_is_anchored_at_the_logical_cell() {
    assert_eq!(IMAGE.iter().map(|byte| byte.count_ones()).sum::<u32>(), 1);
    assert_eq!(IMAGE[31], 0x08);
    assert_eq!(IMAGE[63], 0);
    assert_eq!(COLOR, 1);
    for y in 0..200 {
        for x in 0..160 {
            let origin = vic_position((x, y));
            assert_eq!(origin.0 as i16 + 12 - 24, x * 2);
            assert_eq!(origin.1 as i16 + 10 - 50, y);
            assert_eq!(screen_position(origin), (x, y));
        }
    }
}
