mod cli;
mod dispatch;
mod hero_series;

use clap::Parser;

use cli::Cli;

pub(crate) async fn run() {
    let cli = Cli::parse();
    let code = match dispatch::dispatch(cli).await {
        Ok(c) => c,
        Err(err) => {
            eprintln!("jekko-runner: {err:#}");
            1
        }
    };
    std::process::exit(code);
}
