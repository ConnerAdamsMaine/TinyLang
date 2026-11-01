fn main() -> std::process::ExitCode {
    match tinylang_v2::engine::cli::run(std::env::args().skip(1), &mut std::io::stdout().lock()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
