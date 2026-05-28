#[cfg(target_os = "linux")]
const RPATH: &str = "$ORIGIN/../lib/steam-broker";

fn main() {
    #[cfg(target_os = "linux")]
    println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{RPATH}");
}
