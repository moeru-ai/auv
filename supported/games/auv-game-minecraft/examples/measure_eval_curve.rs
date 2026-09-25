//! Honest photometric measurement of Brush `--eval-save-to-disk` renders.
//!
//! Usage: `measure_eval_curve <gt_images_dir> <training_dir>`
//!
//! For each `--eval-every` step directory `eval_{step}` under `<training_dir>`,
//! compares every `frame_*.png` render against the same-named ground truth PNG
//! in `<gt_images_dir>` using the Slice A PSNR + pure-Rust SSIM implementation
//! ([`compare_png_pair`]). Prints one CSV line per pair:
//! `step,frame,psnr_db,ssim,l1_mean,mse`. Any load or size mismatch aborts with
//! a non-zero exit instead of emitting partial metrics.

use std::path::PathBuf;

use auv_game_minecraft::training_result_holdout_render_quality::compare_png_pair;

fn main() {
  let args: Vec<String> = std::env::args().collect();
  if args.len() != 3 {
    eprintln!("usage: measure_eval_curve <gt_images_dir> <training_dir>");
    std::process::exit(2);
  }
  let gt_dir = PathBuf::from(&args[1]);
  let training_dir = PathBuf::from(&args[2]);

  println!("step,frame,psnr_db,ssim,l1_mean,mse");
  let mut step = 500;
  while step <= 5000 {
    let eval_dir = training_dir.join(format!("eval_{step}"));
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&eval_dir)
      .unwrap_or_else(|error| {
        eprintln!("failed to read {}: {error}", eval_dir.display());
        std::process::exit(1);
      })
      .filter_map(|entry| entry.ok().map(|entry| entry.path()))
      .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("png"))
      .collect();
    entries.sort();
    if entries.is_empty() {
      eprintln!("no PNG renders in {}", eval_dir.display());
      std::process::exit(1);
    }
    for render_path in entries {
      let frame = render_path.file_name().and_then(|name| name.to_str()).unwrap_or("?");
      let gt_path = gt_dir.join(frame);
      match compare_png_pair(&gt_path, &render_path) {
        Ok(metrics) => println!(
          "{step},{frame},{:.4},{:.6},{:.6},{:.4}",
          metrics.psnr.unwrap_or(f64::NAN),
          metrics.ssim.unwrap_or(f64::NAN),
          metrics.l1_mean.unwrap_or(f64::NAN),
          metrics.mse.unwrap_or(f64::NAN),
        ),
        Err(error) => {
          eprintln!("measurement failed at step {step} frame {frame}: {error}");
          std::process::exit(1);
        }
      }
    }
    step += 500;
  }
}
