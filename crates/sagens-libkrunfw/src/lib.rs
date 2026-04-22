#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
include!(concat!(env!("OUT_DIR"), "/generated.rs"));

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub static STATIC_KRUNFW_LINKED: () = ();
