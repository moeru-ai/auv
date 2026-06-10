use std::process::ExitCode;

fn main() -> ExitCode {
  match auv_game_balatro::cli::run_from_env() {
    Ok(()) => ExitCode::SUCCESS,
    Err(error) => {
      eprintln!("{error}");
      ExitCode::FAILURE
    }
  }
}
