//! `cargo run -p scrin-ffi --features cli --bin uniffi-bindgen -- generate --library <cdylib> ...`

fn main() {
    uniffi::uniffi_bindgen_main();
}
