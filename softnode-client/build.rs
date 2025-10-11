fn get_api_url() -> &'static str {
    let env_url = option_env!("SOFTNODE_API_URL_BASE");

    #[cfg(target_arch = "wasm32")]
    {
        env_url.unwrap_or("/api/softnode")
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        env_url.unwrap_or("http://127.0.0.1:8080/api/softnode")
    }
}

fn main() {
    let api_url = get_api_url();
    println!("cargo::rustc-env=SOFTNODE_API_URL_BASE={}", api_url);
}
