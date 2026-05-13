mod cli;

use std::env;
use std::process;

use auv_cli::build_default_runtime;
use auv_cli::model::RunStatus;
use cli::{CliCommand, help_text, parse_cli};

fn main() {
  if let Err(error) = run() {
    eprintln!("error: {error}");
    process::exit(1);
  }
}

fn run() -> Result<(), String> {
  let arguments = env::args().skip(1).collect::<Vec<_>>();
  let command = parse_cli(&arguments)?;
  let project_root =
    env::current_dir().map_err(|error| format!("failed to resolve current directory: {error}"))?;
  let runtime = build_default_runtime(project_root)?;

  match command {
    CliCommand::Help => {
      print!("{}", help_text());
    }
    CliCommand::ListCommands => {
      for command in runtime.list_commands() {
        println!(
          "{} -> {}.{}",
          command.id, command.driver_id, command.operation
        );
        println!("  {}", command.summary);
      }
    }
    CliCommand::ListDrivers => {
      for driver in runtime.list_drivers() {
        println!("{}", driver.id);
        println!("  {}", driver.summary);
        println!("  capabilities: {}", driver.capabilities.join(", "));
        println!("  donor boundary: {}", driver.donor_boundary);
      }
    }
    CliCommand::Invoke(request) => {
      let result = runtime.invoke(request)?;
      println!("runId: {}", result.run_id);
      println!("status: {}", result.status.as_str());
      println!("output: {}", result.output_summary);
      for artifact in &result.artifact_paths {
        println!("artifact: {}", artifact.display());
      }

      if let Some(failure) = &result.failure_message {
        return Err(format!(
          "{} (inspect with `auv-cli inspect {}`)",
          failure, result.run_id
        ));
      }

      if result.status == RunStatus::Failed {
        return Err(format!("run {} finished in failed state", result.run_id));
      }
    }
    CliCommand::Inspect { run_id } => {
      print!("{}", runtime.inspect(&run_id)?);
    }
  }

  Ok(())
}
