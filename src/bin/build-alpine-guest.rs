#[path = "build_alpine_guest/args.rs"]
mod args;
#[path = "build_alpine_guest/builder.rs"]
mod builder;
#[path = "build_alpine_guest/image.rs"]
mod image;
#[path = "build_alpine_guest/packages.rs"]
mod packages;
#[path = "build_alpine_guest/verify.rs"]
mod verify;

const BASE_URL: &str = "https://dl-cdn.alpinelinux.org/alpine/v3.23";
const ALPINE_VERSION: &str = "3.23.4";
const ALPINE_RELEASE_KEY_URL: &str = "https://alpinelinux.org/keys/ncopa.asc";
const EXPECTED_SIGNING_FINGERPRINT: &str = "0482D84022F52DF1C4E7CD43293ACD0907D9495A";
const WANTED_PACKAGES: &[&str] = &[
    "bash",
    "python3",
    "py3-pip",
    "ca-certificates-bundle",
    "busybox-binsh",
    "linux-virt",
];

fn main() -> anyhow::Result<()> {
    let args = args::Args::parse()?;
    let builder = builder::Builder::new()?;
    builder.run(args)
}
