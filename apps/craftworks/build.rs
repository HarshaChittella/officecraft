//! On Windows, links in the .exe's icon and version information (packaging/windows/
//! craftworks.res, made by packaging/windows/make_res.sh). Nothing elsewhere.

fn main() {
    println!("cargo:rerun-if-changed=../../packaging/windows/craftworks.res");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let res = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/windows/craftworks.res");
        if res.is_file() {
            println!("cargo:rustc-link-arg-bins={}", res.display());
        }
    }
}
