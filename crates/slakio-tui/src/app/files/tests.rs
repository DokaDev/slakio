use super::*;

#[test]
fn sizes_read_as_a_file_manager_says_them() {
    assert_eq!(size_words(0), "0 B");
    assert_eq!(size_words(812), "812 B");
    assert_eq!(size_words(999), "999 B");
    assert_eq!(size_words(1000), "1.0 KB");
    assert_eq!(size_words(2048), "2.0 KB");
    assert_eq!(size_words(345_678), "346 KB");
    assert_eq!(size_words(999_960), "1.0 MB");
    assert_eq!(size_words(1_234_567), "1.2 MB");
    assert_eq!(size_words(48_000_000), "48 MB");
    assert_eq!(size_words(53_687_091_200), "54 GB");
    assert_eq!(size_words(u64::MAX), "18446744 TB");
}
