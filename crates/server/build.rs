// Release builds embed `frontend/dist`; rebuild when it changes.
fn main() {
    println!("cargo:rerun-if-changed=../../frontend/dist");
}
