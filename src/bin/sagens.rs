#[tokio::main]
async fn main() {
    match sagens_host::sagens::run().await {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("sagens error: {error}");
            std::process::exit(1);
        }
    }
}
