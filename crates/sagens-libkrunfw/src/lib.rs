#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
include!(concat!(env!("OUT_DIR"), "/generated.rs"));

#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
pub static STATIC_KRUNFW_LINKED: () = ();
