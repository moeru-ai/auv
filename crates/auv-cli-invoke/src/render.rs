use std::io;

use anstream::{AutoStream, ColorChoice};

use crate::{InvokeOutputOptions, InvokeResult, InvokeStatus};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvokeCliOutcome {
  pub exit_code: i32,
}

pub fn render_invoke_result(result: &InvokeResult, options: InvokeOutputOptions) -> Result<InvokeCliOutcome, String> {
  if options.json {
    let mut stdout = io::stdout().lock();
    result.write_json(&mut stdout)?;
  } else {
    let stdout = io::stdout();
    let mut stream = AutoStream::new(stdout.lock(), ColorChoice::Auto);
    result.write_human(&mut stream, options, true)?;
  }
  Ok(InvokeCliOutcome {
    exit_code: match result.status() {
      InvokeStatus::Completed => 0,
      InvokeStatus::Failed => 1,
    },
  })
}
