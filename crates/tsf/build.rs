fn main() {
    let def = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("sweet_tsf.def");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-cdylib-link-arg=/DEF:{}", def.display());
    }
    println!("cargo:rerun-if-changed=sweet_tsf.def");
}
