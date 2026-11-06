use super::{clip_middle, clip_place};

#[test]
fn a_long_place_is_cut_in_its_middle() {
    assert_eq!(clip_middle("#feed-customer-16", 10), "#fee…er-16");
    assert_eq!(clip_middle("#backend", 10), "#backend");
}

#[test]
fn a_cut_place_keeps_the_thread_after_its_mark() {
    assert_eq!(clip_place("#long-threads › ⤷ Thread", 20), "#lon…eads › ⤷ Thread");
    assert_eq!(clip_place("#long-threads › ⤷ Thread", 30), "#long-threads › ⤷ Thread");
    // No room for the thread whole: the place is cut by its middle.
    assert_eq!(clip_place("#long-threads › ⤷ Thread", 10), clip_middle("#long-threads › ⤷ Thread", 10));
}
