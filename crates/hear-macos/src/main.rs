#[cfg(not(target_os = "macos"))]
compile_error!("hear-macos only supports macOS");

fn main() {
    println!("hear-macos scaffold");
}
