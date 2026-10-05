use super::*;

#[test]
fn the_generator_is_fixed_for_a_seed() {
    let a: Vec<u64> = (0..4)
        .map({
            let mut r = Rng::new(7);
            move |_| r.next_u64()
        })
        .collect();
    let mut r = Rng::new(7);
    assert_eq!(a, (0..4).map(|_| r.next_u64()).collect::<Vec<_>>());
    // SplitMix64's published first output for seed 0.
    assert_eq!(Rng::new(0).next_u64(), 0xE220_A839_7B1D_CDAF);
}

#[test]
fn keyed_generators_depend_only_on_their_key_and_index() {
    let first = |k: &str, i| Rng::keyed(1, k, i).next_u64();
    assert_eq!(first("C1", 3), first("C1", 3));
    assert_ne!(first("C1", 3), first("C1", 4));
    assert_ne!(first("C1", 3), first("C2", 3));
    assert_eq!(hash(""), 0xCBF2_9CE4_8422_2325);
    assert_eq!(hash("a"), 0xAF63_DC4C_8601_EC8C);
}

#[test]
fn below_and_chance_stay_in_range() {
    let mut r = Rng::new(3);
    for _ in 0..1000 {
        assert!(r.below(5) < 5);
    }
    assert!(!(0..100).any(|_| r.chance(0)));
    assert!((0..100).all(|_| r.chance(100)));
}
