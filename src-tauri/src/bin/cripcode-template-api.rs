#[tokio::main]
async fn main() {
    if let Err(error) = ship_studio_lib::template_api::serve_from_env().await {
        eprintln!("CripCode Template API failed: {error}");
        std::process::exit(1);
    }
}
