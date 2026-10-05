//! `docs/keybindings.md` is generated from the key map; this fails when the committed file is
//! out of date. `SLAKIO_BLESS=1` writes it instead.

#[test]
fn keybindings_doc_is_up_to_date() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/keybindings.md");
    let want = slakio_tui::keymap::doc::render();
    if std::env::var_os("SLAKIO_BLESS").is_some_and(|v| v == "1") {
        std::fs::write(&path, &want).unwrap();
        return;
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        have == want,
        "docs/keybindings.md is out of date; run `SLAKIO_BLESS=1 cargo test -p slakio-tui --test keybindings_doc`"
    );
}
