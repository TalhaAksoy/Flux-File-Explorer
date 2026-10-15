fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();
    // Optional positional arg: initial directory.
    let start = std::env::args().nth(1).map(std::path::PathBuf::from);
    flux_ui::run(start)
}
