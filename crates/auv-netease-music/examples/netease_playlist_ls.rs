use auv_netease_music::{Inputs, parse_inputs_public, render_human_summary, run_live_scan};

fn main() {
  if let Err(error) = run() {
    eprintln!("{error}");
    std::process::exit(1);
  }
}

fn run() -> Result<(), String> {
  let inputs: Inputs = parse_inputs_public(std::env::args().skip(1).collect())?;
  let scan = run_live_scan(&inputs)?;
  let json = serde_json::to_string_pretty(&scan).map_err(|error| error.to_string())?;
  if let Some(path) = &inputs.json_out {
    std::fs::write(path, &json)
      .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
  }
  if inputs.print_json {
    println!("{json}");
  } else {
    println!("{}", render_human_summary(&scan));
  }
  Ok(())
}
