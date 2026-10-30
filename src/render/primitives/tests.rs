use super::source_over;

#[test]
fn packed_source_over_matches_each_premultiplied_channel_exactly() {
    for alpha in 0..=255u32 {
        for old_alpha in 0..=255u32 {
            let source = (alpha << 24) | ((alpha / 2) << 16) | ((alpha / 3) << 8) | alpha;
            let old =
                (old_alpha << 24) | (old_alpha << 16) | ((old_alpha / 2) << 8) | (old_alpha / 3);
            let expected = [0, 8, 16, 24].into_iter().fold(0, |pixel, shift| {
                pixel
                    | ((((source >> shift) & 255) + ((old >> shift) & 255) * (255 - alpha) / 255)
                        << shift)
            });
            assert_eq!(source_over(old, source), expected);
        }
    }
}
