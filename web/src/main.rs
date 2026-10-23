#[cfg(target_arch = "wasm32")]
fn main() {
    localtt_web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {}
