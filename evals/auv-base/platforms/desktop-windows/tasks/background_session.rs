//! Runs the Windows background-session evaluations: the driver acting on windows the user is
//! not looking at, while an independent watch checks the user's desktop.
//!
//! ```text
//! cargo build --release -p auv-base-evals --bin win32-receiver --bin background-session-eval
//! target/release/background-session-eval single --output evals/auv-base/results/windows-single
//! ```
//!
//! Needs an interactive desktop. Exits non-zero when a check fails.

#[cfg(not(windows))]
fn main() {
  eprintln!("background-session-eval requires Windows");
  std::process::exit(2);
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
  use std::path::PathBuf;

  use auv_base_evals::desktop_windows::background::{self, Options};
  use clap::{Parser, Subcommand};

  #[derive(Parser)]
  #[command(about = "Windows background-session evaluations against native Win32 receivers")]
  struct Cli {
    #[command(subcommand)]
    scenario: Scenario,
  }

  #[derive(Subcommand)]
  enum Scenario {
    /// Diagnostic: which step makes the receiver activate or take the foreground.
    Probe {
      #[arg(long)]
      output: PathBuf,
    },
    /// Diagnostic: does WGC keep delivering fresh frames for a window nobody is looking at?
    Freshness {
      #[arg(long)]
      output: PathBuf,
    },
    /// Diagnostic: does each capture path's frame line up with its bounds after a move or resize?
    Geometry {
      #[arg(long)]
      output: PathBuf,
    },
    /// Diagnostic: only the desktop witness, with no receiver and no session, so injected input
    /// it counts came from another program on the machine.
    Quiet {
      #[arg(long)]
      output: PathBuf,
      #[arg(long, default_value_t = 20.0)]
      minutes: f64,
    },
    /// Phase 2: five background windows operated and watched through one session.
    Five {
      #[arg(long)]
      output: PathBuf,
      /// Act-then-observe rounds over all five windows.
      #[arg(long, default_value_t = 20)]
      rounds: u32,
      /// How many of the five receivers stay uncovered at the top of the Z-order.
      #[arg(long, default_value_t = 2)]
      visible: usize,
    },
    /// Phase 3: the sustained benchmark, with per-minute throughput and a degradation check.
    Bench {
      #[arg(long)]
      output: PathBuf,
      #[arg(long, default_value_t = 10.0)]
      minutes: f64,
      #[arg(long, default_value_t = 0)]
      visible: usize,
      /// Which operations each round sends; `click` and `scroll` split the default.
      #[arg(long, value_enum, default_value_t = auv_base_evals::desktop_windows::sustained::BenchOps::Both)]
      ops: auv_base_evals::desktop_windows::sustained::BenchOps,
    },
    /// Phase 3: fault injection. A window is closed, one killed, one moved and resized, one minimized.
    Faults {
      #[arg(long)]
      output: PathBuf,
      #[arg(long, default_value_t = 10.0)]
      minutes: f64,
      /// Pause between rounds, to keep a long run gentle.
      #[arg(long, default_value_t = 250)]
      round_pause_ms: u64,
    },
    /// Phase 3: real applications, one per toolkit. Which operations each one reacts to.
    Apps {
      #[arg(long)]
      output: PathBuf,
      /// Survey only these applications (charmap, calculator, edge, edge-occlusion-off); all when empty.
      #[arg(long, value_delimiter = ',')]
      only: Vec<String>,
      /// Trials per operation.
      #[arg(long, default_value_t = 4)]
      trials: u32,
    },
    /// The `window.*` invoke commands, run as the real `auv` binary against a covered receiver.
    Window {
      #[arg(long)]
      output: PathBuf,
      /// The `auv` executable to run (build it with `cargo build -p auv-cli`).
      #[arg(long, default_value = "target/debug/auv.exe")]
      auv: PathBuf,
    },
    /// The `window.clickText` invoke command, run as the real `auv` binary against a covered receiver.
    ClickText {
      #[arg(long)]
      output: PathBuf,
      /// The `auv` executable to run (build it with `cargo build -p auv-cli`).
      #[arg(long, default_value = "target/debug/auv.exe")]
      auv: PathBuf,
    },
    /// `screen.findText` and `screen.waitForText`, run as the real `auv` binary against an uncovered receiver.
    Screen {
      #[arg(long)]
      output: PathBuf,
      /// The `auv` executable to run (build it with `cargo build -p auv-cli`).
      #[arg(long, default_value = "target/debug/auv.exe")]
      auv: PathBuf,
    },
    /// The commands that act on the desktop rather than one window, run as the real `auv` binary
    /// against a receiver above ordinary windows. Moves the real pointer and sends real keys, to
    /// the receiver only.
    Foreground {
      #[arg(long)]
      output: PathBuf,
      /// The `auv` executable to run (build it with `cargo build -p auv-cli`).
      #[arg(long, default_value = "target/debug/auv.exe")]
      auv: PathBuf,
      /// Leave the receiver's text box with the desktop's input method, to see what an active IME
      /// makes of key presses. The text after each key command is then recorded, not judged.
      #[arg(long)]
      keep_ime: bool,
    },
    /// Complete owned Win32 GUI workflows through direct Rust, local Invoke and a real Runner.
    /// Uses the real pointer and keyboard; restores the desktop afterwards.
    Workflow {
      #[arg(long)]
      output: PathBuf,
      #[arg(long, default_value = "target/debug/auv.exe")]
      auv: PathBuf,
      #[arg(long, default_value_t = 2)]
      rounds: u32,
      /// Resolve, capture and OCR only. No input; does not accept the full workflow.
      #[arg(long)]
      observe_only: bool,
      /// Focus the text field through UIA instead of clicking its OCR result.
      #[arg(long, conflicts_with = "observe_only")]
      semantic_focus: bool,
      /// Execute only these routes; every selected route writes its own evidence.
      #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        default_value = "rust,invoke,runner"
      )]
      routes: Vec<auv_base_evals::desktop_windows::foreground_commands::workflow::Route>,
    },
    /// Phase 1: one background window.
    Single {
      /// Fresh output directory for the JSONL records and receiver logs.
      #[arg(long)]
      output: PathBuf,
      /// How many times each operation is repeated.
      #[arg(long, default_value_t = 20)]
      repetitions: u32,
      /// Leave the receiver at the top of the Z-order instead of under ordinary windows.
      #[arg(long)]
      visible: bool,
      /// Start the receiver as a piped child of this process: the control that shows how a
      /// descendant of the foreground application behaves. Default: detached from it.
      #[arg(long)]
      piped: bool,
    },
  }

  let result = match Cli::parse().scenario {
    Scenario::Probe { output } => auv_base_evals::desktop_windows::probe::foreground_rights(&Options {
      output,
      repetitions: 1,
      visible: false,
      piped: false,
    }),
    Scenario::Freshness { output } => auv_base_evals::desktop_windows::probe::capture_freshness(&Options {
      output,
      repetitions: 1,
      visible: false,
      piped: false,
    }),
    Scenario::Geometry { output } => auv_base_evals::desktop_windows::probe::capture_geometry(&Options {
      output,
      repetitions: 1,
      visible: false,
      piped: false,
    }),
    Scenario::Quiet { output, minutes } => auv_base_evals::desktop_windows::probe::quiet(&output, minutes),
    Scenario::Five {
      output,
      rounds,
      visible,
    } => auv_base_evals::desktop_windows::sustained::five(&auv_base_evals::desktop_windows::sustained::FiveOptions {
      output,
      rounds,
      visible,
    }),
    Scenario::Bench {
      output,
      minutes,
      visible,
      ops,
    } => auv_base_evals::desktop_windows::sustained::bench(&auv_base_evals::desktop_windows::sustained::BenchOptions {
      output,
      minutes,
      visible,
      ops,
    }),
    Scenario::Faults {
      output,
      minutes,
      round_pause_ms,
    } => auv_base_evals::desktop_windows::sustained::faults(&auv_base_evals::desktop_windows::sustained::FaultOptions {
      output,
      minutes,
      round_pause_ms,
    }),
    Scenario::Apps {
      output,
      only,
      trials,
    } => auv_base_evals::desktop_windows::apps::apps(&auv_base_evals::desktop_windows::apps::AppsOptions {
      output,
      only,
      trials,
    }),
    Scenario::Window { output, auv } => auv_base_evals::desktop_windows::window_commands::window_commands(
      &auv_base_evals::desktop_windows::window_commands::WindowCommandsOptions { output, auv },
    ),
    Scenario::ClickText { output, auv } => auv_base_evals::desktop_windows::window_commands::clicktext_trials(
      &auv_base_evals::desktop_windows::window_commands::WindowCommandsOptions { output, auv },
    ),
    Scenario::Screen { output, auv } => auv_base_evals::desktop_windows::screen_commands::screen_commands(
      &auv_base_evals::desktop_windows::screen_commands::ScreenCommandsOptions { output, auv },
    ),
    Scenario::Foreground {
      output,
      auv,
      keep_ime,
    } => auv_base_evals::desktop_windows::foreground_commands::foreground_commands(
      &auv_base_evals::desktop_windows::foreground_commands::ForegroundCommandsOptions {
        output,
        auv,
        keep_ime,
      },
    ),
    Scenario::Workflow {
      output,
      auv,
      rounds,
      observe_only,
      semantic_focus,
      routes,
    } => auv_base_evals::desktop_windows::foreground_commands::workflow::run(
      &auv_base_evals::desktop_windows::foreground_commands::workflow::Options {
        output,
        auv,
        rounds,
        observe_only,
        semantic_focus,
        routes,
      },
    ),
    Scenario::Single {
      output,
      repetitions,
      visible,
      piped,
    } => background::single(&Options {
      output,
      repetitions,
      visible,
      piped,
    }),
  };
  match result {
    Ok(true) => std::process::ExitCode::SUCCESS,
    Ok(false) => std::process::ExitCode::FAILURE,
    Err(error) => {
      eprintln!("background-session-eval failed: {error:#}");
      std::process::ExitCode::from(2)
    }
  }
}
