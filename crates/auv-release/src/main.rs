//! Builds and verifies AUV release artifacts in CI.
//!
//! NOTICE: this replaces the bash and embedded Python scripts that packaged
//! the Linux bundle. Packaging, verification and the runtime
//! (`auv-driver-linux`) now share one bundle layout (`auv-bundle-layout`) and
//! one set of rules (`rules`), so a path or run-path change cannot reach only
//! some of them. System tools that resolve what the loader and dpkg see
//! (`lddtree`, `ldd`, `dpkg-query`), and that write ELF files (`patchelf`),
//! are still run as subprocesses.

mod elf;
mod ldd;
mod manifest;
mod package;
mod rules;
mod system;
mod verify;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "Builds and verifies AUV release artifacts")]
struct Cli {
  #[command(subcommand)]
  command: Commands,
}

#[derive(Subcommand)]
enum Commands {
  /// Packages a glibc Linux executable as the self-contained release archive.
  PackageLinux {
    #[arg(long)]
    binary: PathBuf,
    #[arg(long)]
    archive: PathBuf,
    #[arg(long)]
    target: String,
    #[arg(long)]
    version: String,
    #[arg(long)]
    project_license: PathBuf,
  },
  /// Checks a Linux release archive against its manifest and the bundle rules.
  VerifyLinuxArchive {
    #[arg(long)]
    archive: PathBuf,
    #[arg(long)]
    target: String,
    #[arg(long)]
    version: String,
  },
  /// Checks that ELF files need no glibc newer than the release baseline.
  VerifyGlibcBaseline {
    #[arg(required = true)]
    elves: Vec<PathBuf>,
  },
  /// Checks a staged Linux npm platform package and what `npm pack` keeps.
  VerifyNpmPackage { package: PathBuf },
}

fn main() -> ExitCode {
  let result = match Cli::parse().command {
    Commands::PackageLinux {
      binary,
      archive,
      target,
      version,
      project_license,
    } => package::run(&package::PackageOptions {
      binary,
      archive,
      target,
      version,
      project_license,
    }),
    Commands::VerifyLinuxArchive {
      archive,
      target,
      version,
    } => verify::linux_archive(&archive, &target, &version),
    Commands::VerifyGlibcBaseline { elves } => verify::glibc_baseline(&elves),
    Commands::VerifyNpmPackage { package } => verify::npm_package(&package),
  };
  match result {
    Ok(()) => ExitCode::SUCCESS,
    Err(error) => {
      eprintln!("error: {error}");
      ExitCode::FAILURE
    }
  }
}
