fn main() {
    let src = std::path::Path::new("src");
    cc::Build::new()
        .include(src)
        .file(src.join("parser.c"))
        // The generated parser is not ours to tidy, and a warning from it
        // is not a defect this project can fix without regenerating.
        .warnings(false)
        .compile("tree-sitter-rue");
    println!("cargo:rerun-if-changed=src/parser.c");
    println!("cargo:rerun-if-changed=grammar.js");
}
