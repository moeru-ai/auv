// Sessions.jsx — three click-through CLI sessions:
//   0. skill cases report — coverage report for the phase-1 narrow skill
//   1. skill run --dry-run — recipe dry-run, step-by-step
//   2. inspect <run_id>  — inspect a finished failed run

function SessionCasesReport() {
  const T = window.AUV_TOKENS
  return (
    <>
      <Comment># coverage truth for the phase-1 narrow QQ音乐 playback skill</Comment>
      <Prompt args="macos.qqmusic.play_visible_anchor.v0" command="cargo run --quiet -- skill cases report" />
      <Blank />
      <KV k="skill" v="macos.qqmusic.play_visible_anchor.v0" vColor={T.brandSoft} />
      <KV k="status" v="validated-recipe" vColor={T.validated} />
      <KV k="strategy" v="playback / ocr-anchor / pointer-double-click  → verifyImageText" />
      <KV k="contract" v="verifyImageText" />
      <KV k="taxonomy" v="playback.ocr-anchor.pointer-double-click.verifyImageText" vColor={T.fg2} />
      <KV k="max disturbance" v="pointer" vColor={T.boundary} />
      <Blank />
      <Out color={T.fg}>cases (4):</Out>
      <Sigil id="ascii-aa-cure-for-me" indent={1} kind="validated" />
      <Sigil id="ascii-aa-soft-universe" indent={1} kind="validated" />
      <Sigil id="ascii-aa-aa-alone-again" indent={1} kind="validated" />
      <Sigil id="chinese-query-chinese-anchor" indent={1} kind="candidate" note="fails at resolve-ocr-anchor, 0 OCR matches for 晴天" />
      <Blank />
      <Out color={T.fg}>validatedClaims (2):</Out>
      <Out color={T.fg2} indent={1}>· ASCII QQ音乐 playback is validated through OCR-anchor activation and</Out>
      <Out color={T.fg2} indent={1}>  evidence-image verification.</Out>
      <Out color={T.fg2} indent={1}>· The anchor-based narrow skill is validated across multiple visible</Out>
      <Out color={T.fg2} indent={1}>  ASCII rows on the aa result page.</Out>
      <Blank />
      <Out color={T.candidate}>boundaryClaims (2):</Out>
      <Out color={T.candidateSoft} indent={1}>· Chinese OCR anchor playback is still a candidate boundary.</Out>
      <Out color={T.candidateSoft} indent={1}>· This member does not prove pointer-free activation or generalized</Out>
      <Out color={T.candidateSoft} indent={1}>  playback for arbitrary queries.</Out>
    </>
  )
}

function SessionDryRun() {
  const T = window.AUV_TOKENS
  return (
    <>
      <Comment># dry-run the formal playback recipe — no desktop is touched</Comment>
      <Prompt args="macos.qqmusic.play_visible_anchor.v0 --dry-run" command="cargo run --quiet -- skill run" />
      <Blank />
      <KV k="recipe" v="macos.qqmusic.play_visible_anchor.v0" vColor={T.brandSoft} />
      <KV k="version" v="0.1.0" />
      <KV k="target_app" v="QQ音乐  (com.tencent.QQMusicMac)" />
      <KV k="dry_run" v="true" vColor={T.candidate} />
      <KV k="max_disturbance (cap)" v="pointer" vColor={T.boundary} />
      <Blank />
      <Out color={T.fg}>steps (8):</Out>
      <Sigil id="open-search                debug.pressKey · cmd+f" indent={1} kind="ok" label="planned" />
      <Sigil id="paste-query                debug.pasteTextPreserveClipboard" indent={1} kind="ok" label="planned" />
      <Sigil id="dismiss-search-overlay     debug.pressKey · escape" indent={1} kind="ok" label="planned" />
      <Sigil id="wait-for-ocr-anchor        debug.waitForScreenText" indent={1} kind="ok" label="planned" />
      <Sigil id="resolve-ocr-anchor         debug.findScreenText" indent={1} kind="ok" label="planned" />
      <Sigil id="double-click-row-anchor    debug.clickScreenText · count=2" indent={1} kind="ok" label="planned" />
      <Sigil id="capture-evidence           debug.captureDisplay" indent={1} kind="ok" label="planned" />
      <Sigil id="verify-player-title        debug.findImageText" indent={1} kind="ok" label="planned" />
      <Blank />
      <KV k="runId" v="run_1778947574511_68037_4" vColor={T.brandSoft} />
      <KV k="status" v="completed (dry-run)" vColor={T.validated} />
      <KV k="output" v="0 desktop events emitted; 0 artifacts written" />
      <Out color={T.fg3}>// re-run without --dry-run to record a live trace under .auv/runs/...</Out>
    </>
  )
}

function SessionInspect() {
  const T = window.AUV_TOKENS
  return (
    <>
      <Comment># inspect a finished failed run — boundary case still reproducible</Comment>
      <Prompt args="run_1778945002311_67885_1" command="cargo run --quiet -- inspect" />
      <Blank />
      <KV k="api_version" v="auv.run.v1alpha1" vColor={T.brandSoft} />
      <KV k="run_id" v="run_1778945002311_67885_1" />
      <KV k="trace_id" v="b22f9c0c1e3d4a607f5b1c2d3e4f5071" vColor={T.fg2} />
      <KV k="run_type" v="execute" />
      <KV k="state" v="ended" />
      <KV k="status_code" v="error" vColor={T.failed} />
      <KV k="recipe_id" v="macos.qqmusic.play_visible_anchor.v0" />
      <KV k="started_at" v="2026-05-20T14:23:22.311Z" vColor={T.fg2} />
      <KV k="finished_at" v="2026-05-20T14:23:25.811Z" vColor={T.fg2} />
      <Blank />
      <Out color={T.fg}>spans (5):</Out>
      <Sigil id="auv.execute" indent={1} kind="ok" label="ok" />
      <Sigil id="auv.recipe.step  step_id=open-search" indent={2} kind="ok" label="ok" />
      <Sigil id="auv.recipe.step  step_id=paste-query" indent={2} kind="ok" label="ok" />
      <Sigil id="auv.recipe.step  step_id=wait-for-ocr-anchor" indent={2} kind="ok" label="ok" />
      <Sigil id="auv.recipe.step  step_id=resolve-ocr-anchor" indent={2} kind="err" label="error" note="ocr.match_found = false" />
      <Blank />
      <Out color={T.fg}>events (12):</Out>
      <Out color={T.fg2} indent={1}>· command.resolved   debug.waitForScreenText</Out>
      <Out color={T.fg2} indent={1}>· driver.invoke      macos.vision-ocr</Out>
      <Out color={T.fg2} indent={1}>· action.started     query="晴天"  region=[0.14, 0.34, 0.90, 0.95]</Out>
      <Out color={T.fg2} indent={1}>· artifact.captured  artifact_0003_screenshot.png</Out>
      <Out color={T.candidateSoft} indent={1}>· assertion.failed   ocr.best_match_text  ≠  "晴天"</Out>
      <Blank />
      <KV k="failure" v={`{ "code": "ocr.zero_matches", "step_id": "resolve-ocr-anchor",`} vColor={T.failed} />
      <Out color={T.failed} indent={1}>{`  "message": "0 ocr matches in constrained result region" }`}</Out>
      <Blank />
      <Out color={T.fg3}>// known boundary — chinese requested-title selection is not yet validated.</Out>
      <Out color={T.fg3}>// see  docs/ai/references/apps/qqmusic/2026-05-17-qqmusic-narrow-skill-coverage.md</Out>
    </>
  )
}

Object.assign(window, { SessionCasesReport, SessionDryRun, SessionInspect })
