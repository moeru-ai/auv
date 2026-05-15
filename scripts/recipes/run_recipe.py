#!/usr/bin/env python3
import argparse
import json
import os
import subprocess
import sys
import time
from contextlib import nullcontext
from pathlib import Path
from string import Template

DISTURBANCE_ORDER = [
  "none",
  "focus",
  "foreground_app",
  "keyboard",
  "clipboard",
  "pointer",
]


class LiveAppRecipeLock:
  def __init__(self, path: Path):
    self.path = path

  def release(self) -> None:
    try:
      self.path.unlink()
    except FileNotFoundError:
      pass

  def __enter__(self):
    return self

  def __exit__(self, exc_type, exc, exc_tb):
    self.release()


def load_recipe(path: Path) -> dict:
  with path.open("r", encoding="utf-8") as handle:
    return json.load(handle)


def parse_set(entries: list[str]) -> dict[str, str]:
  values: dict[str, str] = {}
  for entry in entries:
    if "=" not in entry:
      raise SystemExit(f"invalid --set value {entry!r}; expected key=value")
    key, value = entry.split("=", 1)
    key = key.strip()
    if not key:
      raise SystemExit(f"invalid --set value {entry!r}; missing key")
    values[key] = value
  return values


def default_inputs(recipe: dict) -> dict[str, str]:
  resolved: dict[str, str] = {}
  for key, spec in recipe.get("inputs", {}).items():
    if "default" in spec:
      resolved[key] = stringify(spec["default"])
  return resolved


def stringify(value) -> str:
  if isinstance(value, bool):
    return "true" if value else "false"
  return str(value)


def render_value(raw, variables: dict[str, str]) -> str:
  if isinstance(raw, str):
    return Template(raw).safe_substitute(variables)
  return stringify(raw)


def build_command(step: dict, variables: dict[str, str]) -> list[str]:
  command = ["cargo", "run", "--quiet", "--", "invoke", step["command_id"]]
  args = step.get("args", {})
  if "target" in args:
    command.extend(["--target", render_value(args["target"], variables)])
  for key, value in args.items():
    if key == "target":
      continue
    command.extend([f"--{key}", render_value(value, variables)])
  return command


def sanitize_step_component(raw: str) -> str:
  lowered = raw.strip().lower().replace("-", "_")
  collapsed = "".join(character if (character.isalnum() or character == "_") else "_" for character in lowered)
  while "__" in collapsed:
    collapsed = collapsed.replace("__", "_")
  cleaned = collapsed.strip("_")
  return cleaned or "step"


def parse_invoke_output(output: str) -> dict[str, object]:
  parsed: dict[str, object] = {
    "run_id": "",
    "status": "",
    "output": "",
    "artifacts": [],
  }

  artifacts: list[str] = []
  for raw_line in output.splitlines():
    line = raw_line.strip()
    if line.startswith("runId: "):
      parsed["run_id"] = line.removeprefix("runId: ").strip()
    elif line.startswith("status: "):
      parsed["status"] = line.removeprefix("status: ").strip()
    elif line.startswith("output: "):
      parsed["output"] = line.removeprefix("output: ").strip()
    elif line.startswith("artifact: "):
      artifacts.append(line.removeprefix("artifact: ").strip())

  parsed["artifacts"] = artifacts
  return parsed


def export_step_result_variables(step_id: str, parsed: dict[str, object], variables: dict[str, str]) -> None:
  prefix = f"step_{sanitize_step_component(step_id)}"
  run_id = str(parsed.get("run_id", ""))
  status = str(parsed.get("status", ""))
  output = str(parsed.get("output", ""))
  artifacts = [str(value) for value in parsed.get("artifacts", [])]

  variables[f"{prefix}_run_id"] = run_id
  variables[f"{prefix}_status"] = status
  variables[f"{prefix}_output"] = output
  variables[f"{prefix}_artifact_count"] = str(len(artifacts))

  image_artifacts = [artifact for artifact in artifacts if artifact.lower().endswith((".png", ".jpg", ".jpeg"))]

  for index, artifact in enumerate(artifacts):
    variables[f"{prefix}_artifact_{index}"] = artifact

  if artifacts:
    variables[f"{prefix}_artifact_last"] = artifacts[-1]
  if image_artifacts:
    variables[f"{prefix}_artifact_image_0"] = image_artifacts[0]
    variables[f"{prefix}_artifact_image_last"] = image_artifacts[-1]


def disturbance_rank(name: str) -> int:
  try:
    return DISTURBANCE_ORDER.index(name)
  except ValueError as error:
    raise SystemExit(
      f"unknown disturbance class {name!r}; expected one of {', '.join(DISTURBANCE_ORDER)}"
    ) from error


def validate_disturbance_policy(recipe: dict, max_disturbance: str | None) -> str:
  policy = recipe.get("disturbance_policy", {})
  recipe_max = policy.get("max_disturbance", "pointer")
  active_max = max_disturbance or recipe_max
  declared = set(policy.get("declared_classes", []))

  disturbance_rank(active_max)
  disturbance_rank(recipe_max)

  if disturbance_rank(active_max) > disturbance_rank(recipe_max):
    raise SystemExit(
      f"requested max disturbance {active_max!r} exceeds recipe max disturbance {recipe_max!r}"
    )

  for step in recipe.get("steps", []):
    disturbance = step.get("disturbance", {})
    step_max = disturbance.get("max", "none")
    step_classes = disturbance.get("classes", [])
    disturbance_rank(step_max)
    if disturbance_rank(step_max) > disturbance_rank(active_max):
      raise SystemExit(
        f"step {step['id']!r} requires disturbance {step_max!r}, above allowed max {active_max!r}"
      )
    for step_class in step_classes:
      disturbance_rank(step_class)
      if disturbance_rank(step_class) > disturbance_rank(step_max):
        raise SystemExit(
          f"step {step['id']!r} declares class {step_class!r} above its own max {step_max!r}"
        )
      if declared and step_class not in declared:
        raise SystemExit(
          f"step {step['id']!r} uses class {step_class!r} not declared by recipe policy"
        )

  return active_max


def sanitize_lock_component(raw: str) -> str:
  collapsed = "".join(character if character.isalnum() else "-" for character in raw)
  cleaned = "-".join(segment for segment in collapsed.split("-") if segment)
  return cleaned or "unknown"


def maybe_acquire_live_app_lock(recipe: dict, variables: dict[str, str], dry_run: bool) -> LiveAppRecipeLock | None:
  if dry_run:
    return None

  target_app = recipe.get("target_app", {})
  if target_app.get("display_mode") != "live-desktop":
    return None

  bundle_id = Template(target_app.get("bundle_id", "")).safe_substitute(variables).strip()
  if not bundle_id:
    return None

  timeout_ms = int(os.environ.get("AUV_RECIPE_LOCK_TIMEOUT_MS", "10000"))
  lock_path = Path("/tmp") / f"auv-live-app-{sanitize_lock_component(bundle_id)}.lock"
  started_at = time.monotonic()

  while True:
    try:
      fd = os.open(lock_path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
      with os.fdopen(fd, "w", encoding="utf-8") as handle:
        handle.write(f"pid={os.getpid()}\n")
        handle.write(f"recipe={recipe.get('recipe_id', 'unknown')}\n")
        handle.write(f"bundleId={bundle_id}\n")
      return LiveAppRecipeLock(lock_path)
    except FileExistsError:
      elapsed_ms = int((time.monotonic() - started_at) * 1000)
      if elapsed_ms > timeout_ms:
        raise SystemExit(
          f"timed out waiting for live-app recipe lock for {bundle_id!r} after {timeout_ms} ms"
        )
      time.sleep(0.05)


def run_recipe(
  recipe_path: Path,
  dry_run: bool,
  overrides: dict[str, str],
  max_disturbance: str | None,
) -> int:
  recipe = load_recipe(recipe_path)
  variables = default_inputs(recipe)
  variables.update(overrides)
  active_max_disturbance = validate_disturbance_policy(recipe, max_disturbance)
  lock = maybe_acquire_live_app_lock(recipe, variables, dry_run)

  print(f"recipe: {recipe['recipe_id']}")
  print(f"objective: {recipe['objective']}")
  print(f"target: {Template(recipe['target_app']['bundle_id']).safe_substitute(variables)}")
  print(f"max disturbance: {active_max_disturbance}")

  with lock or nullcontext():
    for index, step in enumerate(recipe.get("steps", []), start=1):
      command = build_command(step, variables)
      disturbance = step.get("disturbance", {})
      step_max = disturbance.get("max", "none")
      step_classes = ", ".join(disturbance.get("classes", [])) or "none"
      print(
        f"[{index}/{len(recipe['steps'])}] {step['id']} "
        f"(disturbance max={step_max}; classes={step_classes}) -> {' '.join(command)}"
      )
      if dry_run:
        continue
      completed = subprocess.run(command, check=True, capture_output=True, text=True)
      if completed.stdout:
        print(completed.stdout, end="")
      if completed.stderr:
        print(completed.stderr, end="", file=sys.stderr)
      export_step_result_variables(step["id"], parse_invoke_output(completed.stdout), variables)

  return 0


def main() -> int:
  parser = argparse.ArgumentParser(
    description="Replay an AUV recipe manifest through the current runtime.",
  )
  parser.add_argument("recipe", help="Path to the recipe JSON file.")
  parser.add_argument(
    "--set",
    action="append",
    default=[],
    metavar="KEY=VALUE",
    help="Override a recipe input.",
  )
  parser.add_argument(
    "--dry-run",
    action="store_true",
    help="Print the resolved commands without executing them.",
  )
  parser.add_argument(
    "--max-disturbance",
    choices=DISTURBANCE_ORDER,
    help="Restrict execution to recipe steps at or below this disturbance level.",
  )
  arguments = parser.parse_args()

  recipe_path = Path(arguments.recipe).resolve()
  if not recipe_path.exists():
    raise SystemExit(f"recipe file not found: {recipe_path}")

  os.chdir(Path(__file__).resolve().parents[2])
  return run_recipe(
    recipe_path,
    arguments.dry_run,
    parse_set(arguments.set),
    arguments.max_disturbance,
  )


if __name__ == "__main__":
  raise SystemExit(main())
