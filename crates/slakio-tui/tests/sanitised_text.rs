//! Remote text reaches the screen only through the sanitiser. The type does most of it (a
//! `Remote` cannot be formatted or used as a string); this keeps the one named way around it,
//! `Remote::unsanitized`, and `Safe::trusted` out of the drawing code.

use std::path::Path;

fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push((p.display().to_string(), std::fs::read_to_string(&p).unwrap()));
        }
    }
}

#[test]
fn drawing_code_never_takes_remote_text_around_the_sanitiser() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![];
    sources(&src.join("ui"), &mut files);
    files.push(("ui.rs".into(), std::fs::read_to_string(src.join("ui.rs")).unwrap()));
    assert!(files.len() > 3, "the drawing code was found");
    for (name, text) in files {
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            assert!(
                !code.contains(".unsanitized(") && !code.contains("Safe::trusted("),
                "{name}:{}: remote text drawn around the sanitiser: {line}",
                n + 1
            );
        }
    }
}
