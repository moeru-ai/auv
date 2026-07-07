use std::fs;
use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

pub type PrepResult<T> = Result<T, String>;

pub const TEXTURE_SWEEP_PREP_SCHEMA_VERSION: u32 = 1;
pub const MINECRAFT_1_21_1_RESOURCE_PACK_FORMAT: u32 = 34;
pub const TEXTURE_SWEEP_PROFILE_DURATION_SECONDS: f64 = 30.0;

const PROFILE_PACKS: [(&str, &str, &str); 3] = [
  ("rich", "auv-mc6-rich", "AUV MC-6 rich texture sweep profile"),
  ("flat_color", "auv-mc6-flat-color", "AUV MC-6 flat-color texture sweep profile"),
  ("repetitive", "auv-mc6-repetitive", "AUV MC-6 repetitive texture sweep profile"),
];

const BLOCK_TEXTURES: [&str; 10] = [
  "stone",
  "dirt",
  "grass_block_top",
  "grass_block_side",
  "cobblestone",
  "oak_planks",
  "oak_log",
  "oak_log_top",
  "netherrack",
  "oak_button",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextureSweepPreparationInputs {
  pub sidecar_run_dir: PathBuf,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureSweepPreparationOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub runbook_path: PathBuf,
  pub manifest: TextureSweepPreparationManifest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureSweepPreparationManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub sidecar_run_dir: String,
  pub resourcepacks_dir: String,
  pub pack_format: u32,
  pub profiles: Vec<TextureSweepPreparedProfile>,
  pub live_run_sequence: Vec<TextureSweepRunStep>,
  pub final_eval_command: String,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureSweepPreparedProfile {
  pub texture_profile: String,
  pub pack_id: String,
  pub pack_dir: String,
  pub options_resource_packs_value: String,
  pub expected_telemetry_resource_pack_id: String,
  pub required_duration_seconds: f64,
  pub texture_overrides: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureSweepRunStep {
  pub texture_profile: String,
  pub options_resource_packs_value: String,
  pub expected_telemetry_resource_pack_id: String,
  pub acceptance_note: String,
}

pub fn prepare_texture_sweep_resource_packs(inputs: TextureSweepPreparationInputs) -> PrepResult<TextureSweepPreparationOutput> {
  let resourcepacks_dir = inputs.sidecar_run_dir.join("resourcepacks");
  fs::create_dir_all(&resourcepacks_dir)
    .map_err(|error| format!("failed to create Minecraft resourcepacks directory {}: {error}", resourcepacks_dir.display()))?;
  fs::create_dir_all(&inputs.output_dir)
    .map_err(|error| format!("failed to create MC-6 preparation output directory {}: {error}", inputs.output_dir.display()))?;

  let mut profiles = Vec::new();
  for (profile, pack_id, description) in PROFILE_PACKS {
    let pack_dir = resourcepacks_dir.join(pack_id);
    write_resource_pack(&pack_dir, profile, description)?;
    profiles.push(TextureSweepPreparedProfile {
      texture_profile: profile.to_string(),
      pack_id: pack_id.to_string(),
      pack_dir: pack_dir.to_string_lossy().into_owned(),
      options_resource_packs_value: options_resource_packs_value(pack_id),
      expected_telemetry_resource_pack_id: format!("file/{pack_id}"),
      required_duration_seconds: TEXTURE_SWEEP_PROFILE_DURATION_SECONDS,
      texture_overrides: BLOCK_TEXTURES.iter().map(|name| format!("assets/minecraft/textures/block/{name}.png")).collect(),
    });
  }

  let live_run_sequence = profiles
    .iter()
    .map(|profile| TextureSweepRunStep {
      texture_profile: profile.texture_profile.clone(),
      options_resource_packs_value: profile.options_resource_packs_value.clone(),
      expected_telemetry_resource_pack_id: profile.expected_telemetry_resource_pack_id.clone(),
      acceptance_note: format!(
        "collect at least {:.0}s of in_game telemetry plus at least one exercised refusal before export",
        profile.required_duration_seconds
      ),
    })
    .collect::<Vec<_>>();
  let manifest = TextureSweepPreparationManifest {
    schema_version: TEXTURE_SWEEP_PREP_SCHEMA_VERSION,
    generated_at_millis: auv_tracing_driver::now_millis(),
    sidecar_run_dir: inputs.sidecar_run_dir.to_string_lossy().into_owned(),
    resourcepacks_dir: resourcepacks_dir.to_string_lossy().into_owned(),
    pack_format: MINECRAFT_1_21_1_RESOURCE_PACK_FORMAT,
    profiles,
    live_run_sequence,
    final_eval_command:
      "auv-cli minecraft eval-texture-sweep --samples <real-samples.json> --output-dir <dir> --require-real-source --store-root .auv --inspect-server-write false"
        .to_string(),
    known_limits: vec![
      "preparation only: this command does not launch Minecraft, collect frames, export bundles, or close MC-6 numerically".to_string(),
      "pack_format=34 is the Minecraft 1.21.1 client resource-pack version observed from local SharedConstants.RESOURCE_PACK_VERSION".to_string(),
      "enable exactly one MC-6 resource pack per live collection run; the sample builder treats multiple non-default pack ids in one bundle as invalid".to_string(),
    ],
  };
  let manifest_path = inputs.output_dir.join("mc6-texture-sweep-prep.json");
  write_json(&manifest_path, &manifest)?;
  let runbook_path = inputs.output_dir.join("mc6-texture-sweep-runbook.md");
  fs::write(&runbook_path, render_runbook(&manifest).as_bytes())
    .map_err(|error| format!("failed to write MC-6 texture sweep runbook {}: {error}", runbook_path.display()))?;

  Ok(TextureSweepPreparationOutput {
    output_dir: inputs.output_dir,
    manifest_path,
    runbook_path,
    manifest,
  })
}

fn write_resource_pack(pack_dir: &Path, profile: &str, description: &str) -> PrepResult<()> {
  if pack_dir.exists() {
    fs::remove_dir_all(pack_dir)
      .map_err(|error| format!("failed to refresh MC-6 resource pack directory {}: {error}", pack_dir.display()))?;
  }
  let textures_dir = pack_dir.join("assets/minecraft/textures/block");
  fs::create_dir_all(&textures_dir)
    .map_err(|error| format!("failed to create MC-6 resource pack texture directory {}: {error}", textures_dir.display()))?;
  let pack_meta = serde_json::json!({
    "pack": {
      "pack_format": MINECRAFT_1_21_1_RESOURCE_PACK_FORMAT,
      "description": description,
    }
  });
  write_json(&pack_dir.join("pack.mcmeta"), &pack_meta)?;
  write_texture(&pack_dir.join("pack.png"), profile, 255)?;
  for (index, texture_name) in BLOCK_TEXTURES.iter().enumerate() {
    write_texture(&textures_dir.join(format!("{texture_name}.png")), profile, index as u8)?;
  }
  Ok(())
}

fn write_texture(path: &Path, profile: &str, seed: u8) -> PrepResult<()> {
  let mut image = RgbaImage::new(16, 16);
  for y in 0..16 {
    for x in 0..16 {
      image.put_pixel(x, y, pixel_for(profile, seed, x as u8, y as u8));
    }
  }
  image.save(path).map_err(|error| format!("failed to write texture {}: {error}", path.display()))
}

fn pixel_for(profile: &str, seed: u8, x: u8, y: u8) -> Rgba<u8> {
  match profile {
    "rich" => Rgba([
      seed.wrapping_mul(31).wrapping_add(x.wrapping_mul(13)),
      48u8.wrapping_add(y.wrapping_mul(11)).wrapping_add(seed),
      96u8.wrapping_add(x.wrapping_mul(y.wrapping_add(1))),
      255,
    ]),
    "flat_color" => {
      let base = seed.wrapping_mul(37).wrapping_add(64);
      Rgba([base, base.wrapping_add(18), base.wrapping_add(36), 255])
    }
    "repetitive" => {
      let stripe = if (x / 2).wrapping_add(y / 2).wrapping_add(seed) % 2 == 0 {
        32
      } else {
        216
      };
      Rgba([
        stripe,
        255u8.wrapping_sub(stripe / 2),
        seed.wrapping_mul(19),
        255,
      ])
    }
    _ => Rgba([255, 0, 255, 255]),
  }
}

fn options_resource_packs_value(pack_id: &str) -> String {
  format!("[\"fabric\",\"file/{pack_id}\"]")
}

fn render_runbook(manifest: &TextureSweepPreparationManifest) -> String {
  let mut output = String::new();
  output.push_str("# MC-6 texture sweep runbook\n\n");
  output.push_str("This is a preparation artifact. It does not prove MC-6 closure.\n\n");
  output.push_str("For each profile, set `devtools/auv-game-minecraft/run/options.txt` to the listed `resourcePacks` value, launch the Fabric client manually, collect at least 30 seconds of in-game telemetry, and exercise at least one refusal frame before exporting the source run.\n\n");
  for step in &manifest.live_run_sequence {
    output.push_str(&format!(
      "- `{}`: `resourcePacks:{}`; expect telemetry id `{}`.\n",
      step.texture_profile, step.options_resource_packs_value, step.expected_telemetry_resource_pack_id
    ));
  }
  output.push_str("\nAfter all three source runs are exported as spatial bundles, build the sample file and evaluate with:\n\n");
  output.push_str("```bash\n");
  output.push_str(&manifest.final_eval_command);
  output.push('\n');
  output.push_str("```\n");
  output
}

fn write_json(path: &Path, value: &impl Serialize) -> PrepResult<()> {
  let json = serde_json::to_string_pretty(value)
    .map(|mut json| {
      json.push('\n');
      json
    })
    .map_err(|error| format!("failed to serialize MC-6 preparation JSON: {error}"))?;
  fs::write(path, json.as_bytes()).map_err(|error| format!("failed to write MC-6 preparation JSON {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn prepares_three_texture_profile_resource_packs() {
    let temp = tempfile::tempdir().expect("temp dir");
    let sidecar_run_dir = temp.path().join("run");
    let output_dir = temp.path().join("prep");

    let output = prepare_texture_sweep_resource_packs(TextureSweepPreparationInputs {
      sidecar_run_dir: sidecar_run_dir.clone(),
      output_dir: output_dir.clone(),
    })
    .expect("prep should succeed");

    assert_eq!(output.manifest.schema_version, 1);
    assert_eq!(output.manifest.pack_format, 34);
    assert_eq!(output.manifest.profiles.len(), 3);
    for profile in &output.manifest.profiles {
      let pack_dir = PathBuf::from(&profile.pack_dir);
      assert!(pack_dir.join("pack.mcmeta").is_file());
      assert!(pack_dir.join("pack.png").is_file());
      assert!(pack_dir.join("assets/minecraft/textures/block/stone.png").is_file());
      assert!(profile.options_resource_packs_value.contains(&format!("file/{}", profile.pack_id)));
    }
    assert!(output_dir.join("mc6-texture-sweep-prep.json").is_file());
    assert!(output_dir.join("mc6-texture-sweep-runbook.md").is_file());
  }
}
