pub(crate) fn sanitized_bundle_package_name(raw: &str) -> String {
  let lowered = raw.trim().to_lowercase();
  let collapsed = lowered
    .chars()
    .map(|character| {
      if character.is_ascii_alphanumeric() {
        character
      } else {
        '-'
      }
    })
    .collect::<String>();
  let trimmed = collapsed
    .split('-')
    .filter(|segment| !segment.is_empty())
    .collect::<Vec<_>>()
    .join("-");
  if trimmed.is_empty() {
    "bundle-export".to_string()
  } else {
    trimmed
  }
}

pub(crate) fn bundle_member_relative_dir(recipe_id: &str) -> String {
  format!("members/{}", sanitized_bundle_package_name(recipe_id))
}

pub(crate) fn bundle_member_recipe_relative_path(recipe_id: &str) -> String {
  format!("{}/recipe.json", bundle_member_relative_dir(recipe_id))
}

pub(crate) fn bundle_member_cases_relative_path(recipe_id: &str) -> String {
  format!("{}/cases.json", bundle_member_relative_dir(recipe_id))
}

pub(crate) fn bundle_member_evidence_relative_dir(recipe_id: &str) -> String {
  format!("{}/evidence", bundle_member_relative_dir(recipe_id))
}

pub(crate) fn bundle_member_evidence_relative_path(recipe_id: &str) -> String {
  format!("{}/evidence.txt", bundle_member_relative_dir(recipe_id))
}

pub(crate) fn bundle_member_summary_relative_path(recipe_id: &str) -> String {
  format!("{}/summary.txt", bundle_member_relative_dir(recipe_id))
}

pub(crate) fn bundle_member_coverage_relative_path(recipe_id: &str) -> String {
  format!("{}/coverage.md", bundle_member_relative_dir(recipe_id))
}
