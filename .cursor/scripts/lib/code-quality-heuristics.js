'use strict';

const fs = require('fs');
const path = require('path');
const { getContractIndex } = require('./contract-index');

const SKIP_PATH_RE = /(?:^|\/)(?:node_modules|target|\.git|dist|build)\//;

const FIXTURE_SIGNALS = [
  'dummy_run',
  'stage_json_artifact',
  'LocalStore::new',
  'persist_run',
  'persist_',
  'TempDir::new',
  'tempdir',
  'stage_artifact',
];

const ENTRYPOINT_RE = /(?:^|\/)(?:handler|main|cli|dispatch)\.rs$/;
const ENTRY_FN_RE = /(?:pub\s+)?(?:async\s+)?fn\s+(invoke|handle|main|run|dispatch|execute)\b/;

const RESPONSIBILITY_STAGES = [
  { key: 'parse', pattern: /\b(parse[_\w]*|from_str|deserialize|serde_json::from)/ },
  { key: 'dispatch', pattern: /\b(dispatch[_\w]*|route[_\w]*|match\s+command|invoke_command)/ },
  { key: 'persist', pattern: /\b(persist[_\w]*|\bwrite[_\w]*|\bsave[_\w]*|store\.|upsert|insert)/ },
  { key: 'cache', pattern: /\b(cache|memo|cached|lru|HashMap<)/i },
  { key: 'join', pattern: /\b(join[_\w]*|merge[_\w]*|combine|zip\(|collect::<Vec)/ },
  { key: 'fallback', pattern: /\b(fallback|or_else|unwrap_or|default\(\))/ },
  { key: 'format', pattern: /\b(format!|to_string|serialize|json!|Response::)/ },
  { key: 'validate', pattern: /\b(validate[_\w]*|ensure[_\w]*|check_|assert_)/ },
  { key: 'load', pattern: /\b(load[_\w]*|read[_\w]*|fetch[_\w]*|\.get\(|find_)/ },
  { key: 'transform', pattern: /\b(map\(|map_err|transform[_\w]*|convert[_\w]*|into_)/ },
];

const TEMP_PATH_RE = /(?:\.staging-|\/staging\/|tmp-|temp-|\.tmp\b|\/tmp\/)/i;
const DURABLE_DIR_RE = /(?:store|run_root|artifact|persist|data_dir|summary_store)/i;

const DOC_SIGNAL_RE = /(?:^|\n)\s*(?:\/\/\s*)?(?:NOTICE:|TODO\(|handoff|##\s|\/\/!\s)/m;

function normalizeRel(filePath, repoRoot = process.cwd()) {
  return path.relative(repoRoot, path.resolve(filePath)).replace(/\\/g, '/');
}

function isDocsOnlyPath(rel) {
  if (!rel) return false;
  if (/^docs\//.test(rel)) return true;
  if (rel.endsWith('.md') && !rel.startsWith('.cursor/')) return true;
  return false;
}

function isDocsOnlySession(sessionPaths) {
  const paths = (sessionPaths || []).map(p => normalizeRel(p));
  if (paths.length === 0) return false;
  return paths.every(isDocsOnlyPath);
}

function isTestFile(rel) {
  return /\/tests?\//.test(rel) || /_test\.rs$/.test(rel) || /test_fixtures/.test(rel) || /#\[test\]/.test(rel);
}

function readFileSafe(filePath) {
  try {
    return fs.readFileSync(path.resolve(filePath), 'utf8');
  } catch {
    return '';
  }
}

function normalizeLine(line) {
  return String(line || '')
    .trim()
    .replace(/\s+/g, ' ')
    .replace(/"[^"]*"/g, '"…"')
    .replace(/'[^']*'/g, "'…'");
}

function extractTestBlocks(content) {
  const blocks = [];
  const re = /#\[(?:tokio::)?test\][\s\S]*?(?=\n\s*#\[|\n\s*mod\s+|\n\s*}\s*$|$)/g;
  let match;
  while ((match = re.exec(content)) !== null) {
    const body = match[0];
    const fnMatch = body.match(/fn\s+(\w+)/);
    blocks.push({
      name: fnMatch ? fnMatch[1] : 'unknown',
      body,
      lines: body.split('\n').map(normalizeLine).filter(Boolean),
    });
  }
  return blocks;
}

function fixtureLines(lines) {
  return lines.filter(line => FIXTURE_SIGNALS.some(sig => line.includes(sig)));
}

function jaccardSimilarity(a, b) {
  const setA = new Set(a);
  const setB = new Set(b);
  if (setA.size === 0 || setB.size === 0) return 0;
  let inter = 0;
  for (const item of setA) {
    if (setB.has(item)) inter += 1;
  }
  const union = setA.size + setB.size - inter;
  return union === 0 ? 0 : inter / union;
}

function makeFinding({ code, severity, file, evidence, why_it_matters, suggested_action }) {
  return {
    code,
    severity,
    file,
    evidence,
    why_it_matters,
    suggested_action,
    message: evidence,
  };
}

function analyzeFixtureDuplication(filePath, content, sessionPaths, repoRoot) {
  const rel = normalizeRel(filePath, repoRoot);
  const findings = [];
  if (!/\.rs$/.test(rel)) return findings;
  if (!isTestFile(rel) && !/#\[test\]/.test(content)) return findings;

  const blocks = extractTestBlocks(content);
  const fixtureBlocks = blocks
    .map(block => ({ ...block, fixture: fixtureLines(block.lines) }))
    .filter(block => block.fixture.length >= 3);

  for (let i = 0; i < fixtureBlocks.length; i += 1) {
    for (let j = i + 1; j < fixtureBlocks.length; j += 1) {
      const sim = jaccardSimilarity(fixtureBlocks[i].fixture, fixtureBlocks[j].fixture);
      if (sim >= 0.55) {
        findings.push(makeFinding({
          code: 'fixture-duplication',
          severity: 'medium',
          file: rel,
          evidence: `${fixtureBlocks[i].name} 与 ${fixtureBlocks[j].name} 共享 staging 形状 (${Math.round(sim * 100)}% 行重叠: ${fixtureBlocks[i].fixture.slice(0, 2).join('; ')})`,
          why_it_matters: 'artifact shape 一变会多点同步修改',
          suggested_action: '抽一个测试专用 staging helper，先收同模块内重复',
        }));
        break;
      }
    }
    if (findings.length > 0) break;
  }

  const crossTargets = (sessionPaths || [])
    .map(p => normalizeRel(p, repoRoot))
    .filter(p => p !== rel && /\.rs$/.test(p) && (isTestFile(p) || p.includes('test')));

  if (fixtureBlocks.length > 0 && crossTargets.length > 0) {
    const localSig = fixtureBlocks[0].fixture.slice(0, 6).join('|');
    for (const otherPath of crossTargets) {
      const otherContent = readFileSafe(path.join(repoRoot, otherPath));
      const otherFixture = fixtureLines(otherContent.split('\n').map(normalizeLine).filter(Boolean));
      const sim = jaccardSimilarity(fixtureBlocks[0].fixture, otherFixture);
      if (sim >= 0.5 && otherFixture.length >= 3) {
        findings.push(makeFinding({
          code: 'fixture-duplication',
          severity: 'high',
          file: rel,
          evidence: `与 ${otherPath} 的 staging 夹具形状高度相似 (${Math.round(sim * 100)}%): ${localSig.slice(0, 120)}`,
          why_it_matters: '跨文件复制夹具会在 contract 演进时多点漂移',
          suggested_action: '提到共享 test_fixtures 模块或 crate 内单一 staging owner',
        }));
        break;
      }
    }
  }

  return findings;
}

function countStages(text) {
  const hits = new Set();
  for (const stage of RESPONSIBILITY_STAGES) {
    if (stage.pattern.test(text)) {
      hits.add(stage.key);
    }
  }
  return hits;
}

function braceDeltaOutsideStrings(line) {
  let inStr = false;
  let quote = '';
  let open = 0;
  let close = 0;
  for (let i = 0; i < line.length; i += 1) {
    const c = line[i];
    if (!inStr && (c === '"' || c === "'")) {
      inStr = true;
      quote = c;
      continue;
    }
    if (inStr) {
      if (c === quote && line[i - 1] !== '\\') {
        inStr = false;
      }
      continue;
    }
    if (c === '{') open += 1;
    if (c === '}') close += 1;
  }
  return { open, close };
}

function extractEntryFunctions(content) {
  const fns = [];
  const lines = content.split('\n');
  let collecting = false;
  let depth = 0;
  let name = '';
  const buf = [];

  for (const line of lines) {
    if (!collecting) {
      const fnStart = line.match(/(?:pub\s+)?(?:async\s+)?fn\s+(invoke|handle|main|run|dispatch|execute)\b/);
      if (!fnStart) continue;
      collecting = true;
      name = fnStart[1];
      buf.length = 0;
      depth = 0;
    }

    buf.push(line);
    const delta = braceDeltaOutsideStrings(line);
    depth += delta.open - delta.close;
    if (collecting && depth <= 0 && delta.open + delta.close > 0) {
      fns.push({ name, body: buf.join('\n') });
      collecting = false;
      depth = 0;
      name = '';
      buf.length = 0;
    }
  }

  return fns;
}

function analyzeEntrypointCreep(filePath, content, edits, repoRoot) {
  const rel = normalizeRel(filePath, repoRoot);
  const findings = [];
  if (!ENTRYPOINT_RE.test(rel) && !ENTRY_FN_RE.test(content)) return findings;

  const added = (edits || [])
    .map(edit => String(edit?.new_string || ''))
    .join('\n');

  if (added.trim()) {
    const addedStages = countStages(added);
    if (addedStages.size >= 3) {
      findings.push(makeFinding({
        code: 'entrypoint-responsibility-creep',
        severity: addedStages.size >= 4 ? 'high' : 'medium',
        file: rel,
        evidence: `本次 diff 在入口附近新增 ${addedStages.size} 类职责: ${[...addedStages].join(', ')}`,
        why_it_matters: '入口函数正在积累跨边界职责',
        suggested_action: '下次再加一步时优先抽 owner helper，而不是继续堆进入口',
      }));
    }
  }

  for (const fn of extractEntryFunctions(content)) {
    const stages = countStages(fn.body);
    if (stages.size >= 5) {
      findings.push(makeFinding({
        code: 'entrypoint-responsibility-creep',
        severity: 'high',
        file: rel,
        evidence: `${fn.name}() 同时承载 ${stages.size} 类阶段: ${[...stages].join(', ')}`,
        why_it_matters: '总控大函数会让 replay/测试/边界演进都变脆',
        suggested_action: '按 persist/cache/join 等边界拆私有 helper，入口只编排',
      }));
      break;
    }
  }

  return findings;
}

function analyzeDuplicateContract(filePath, content, edits, repoRoot) {
  const rel = normalizeRel(filePath, repoRoot);
  const findings = [];
  const index = getContractIndex(repoRoot);
  const owner = index.ownerFiles.has(rel);

  const scanText = [
    ...(edits || []).map(edit => String(edit?.new_string || '')),
    content,
  ].join('\n');

  const constRe = /const\s+([A-Z0-9_]+)\s*:\s*&str\s*=\s*"([^"]+)"/g;
  let match;
  while ((match = constRe.exec(scanText)) !== null) {
    const [, name, value] = match;
    if (!/_API_VERSION$|_ARTIFACT_ROLE$|_STATUS$|_LABEL$/.test(name)) continue;
    if (owner) continue;

    const byName = index.byName.get(name);
    const byValue = (index.byValue.get(value) || []).filter(e => e.file !== rel);

    if (byName && byName.file !== rel) {
      findings.push(makeFinding({
        code: 'duplicate-contract-ownership',
        severity: 'high',
        file: rel,
        evidence: `新增 ${name} 与 ${byName.file} 同名 contract 常量`,
        why_it_matters: '双份 vocabulary 会在 wire/artifact 演进时漂移',
        suggested_action: `改为 use crate::contract::${name} 或从 owning crate 导入`,
      }));
      continue;
    }

    if (byValue.length > 0) {
      findings.push(makeFinding({
        code: 'duplicate-contract-ownership',
        severity: 'medium',
        file: rel,
        evidence: `新增 ${name}="${value}" 与 ${byValue[0].file}::${byValue[0].name} 同值`,
        why_it_matters: '同义重复定义会让未来改 version/role 时漏改一处',
        suggested_action: '删除本地副本，引用 contract owner',
      }));
    }
  }

  return findings;
}

function hasCleanupNear(text, index) {
  const window = text.slice(Math.max(0, index - 400), Math.min(text.length, index + 400));
  return /\b(remove|cleanup|clean_up|drop\(|unlink|delete|#[\[]ignore[\]]|NOTICE:.*remov)/i.test(window);
}

function analyzeRoughTempPersistence(filePath, content, edits, repoRoot) {
  const rel = normalizeRel(filePath, repoRoot);
  const findings = [];
  const scanText = [
    ...(edits || []).map(edit => String(edit?.new_string || '')),
    content,
  ].join('\n');

  const pathRe = /["']([^"']*(?:\.staging-|tmp-|temp-|\.tmp)[^"']*)["']/gi;
  let match;
  while ((match = pathRe.exec(scanText)) !== null) {
    const tempPath = match[1];
    if (!DURABLE_DIR_RE.test(scanText) && !/(store|run|artifact|persist)/i.test(tempPath)) {
      continue;
    }
    const hasNotice = /NOTICE:/.test(scanText);
    const hasCleanup = hasCleanupNear(scanText, match.index);
    let severity = 'medium';
    if (hasNotice && hasCleanup) severity = 'low';
    else if (!hasNotice && !hasCleanup) severity = 'medium';

    findings.push(makeFinding({
      code: 'rough-temp-persistence',
      severity,
      file: rel,
      evidence: `临时/暂存路径 "${tempPath}" 落在持久化语境${hasNotice ? ' (有 NOTICE)' : ' (无 NOTICE)'}${hasCleanup ? ', 有清理线索' : ', 未见清理边界'}`,
      why_it_matters: '功能先落下了，持久化边界还没收干净',
      suggested_action: '加 NOTICE 写明 removal condition，或抽到带 drop guard 的 staging helper',
    }));
    break;
  }

  return findings;
}

function analyzeDocsCleanerThanCode(filePath, content, edits, otherFindings, repoRoot) {
  const rel = normalizeRel(filePath, repoRoot);
  if (otherFindings.length === 0) return [];

  const added = (edits || [])
    .map(edit => `${edit?.old_string || ''}\n${edit?.new_string || ''}`)
    .join('\n');

  const docLines = (added.match(/(?:NOTICE:|TODO\(|handoff|\/\/!|##\s)/g) || []).length;
  const moduleDoc = (content.match(/\/\/!|\/\*\*/g) || []).length;

  if (docLines < 2 && moduleDoc < 2) return [];

  return [makeFinding({
    code: 'docs-cleaner-than-code',
    severity: 'medium',
    file: rel,
    evidence: `注释/handoff 在增强，但同文件仍有 ${otherFindings.map(f => f.code).join(', ')}`,
    why_it_matters: '叙事比结构成熟时，后续读者会高估实现边界',
    suggested_action: '先收一处结构信号（夹具/入口/contract），再补文档',
  })];
}

function shouldReview(filePath, repoRoot = process.cwd()) {
  const rel = normalizeRel(filePath, repoRoot);
  if (!rel || rel.startsWith('..')) return false;
  if (SKIP_PATH_RE.test(rel)) return false;
  return /\.(rs|toml|js|ts|md)$/.test(rel) || rel.startsWith('src/') || rel.startsWith('crates/');
}

function dedupeFindings(findings) {
  const unique = [];
  const seen = new Set();
  for (const finding of findings) {
    const key = `${finding.code}|${finding.file}|${finding.evidence}`;
    if (seen.has(key)) continue;
    seen.add(key);
    unique.push(finding);
  }
  return unique;
}

function reviewEditedFile({ filePath, edits, sessionPaths, repoRoot = process.cwd() }) {
  const rel = normalizeRel(filePath, repoRoot);
  const content = readFileSafe(filePath);
  const docsOnly = isDocsOnlySession([...sessionPaths, filePath]);

  let findings = [];

  if (!docsOnly) {
    findings.push(
      ...analyzeFixtureDuplication(filePath, content, sessionPaths, repoRoot),
      ...analyzeEntrypointCreep(filePath, content, edits, repoRoot),
      ...analyzeDuplicateContract(filePath, content, edits, repoRoot),
      ...analyzeRoughTempPersistence(filePath, content, edits, repoRoot),
    );
  }

  if (!docsOnly) {
    findings.push(...analyzeDocsCleanerThanCode(filePath, content, edits, findings, repoRoot));
  }

  findings = dedupeFindings(findings);

  if (docsOnly) {
    findings = findings.filter(f => f.severity !== 'high');
  }

  return {
    filePath: rel,
    findings,
    docsOnly,
  };
}

module.exports = {
  shouldReview,
  reviewEditedFile,
  readFileSafe,
  normalizeRel,
  isDocsOnlySession,
  analyzeFixtureDuplication,
  analyzeEntrypointCreep,
  analyzeDuplicateContract,
  analyzeRoughTempPersistence,
  analyzeDocsCleanerThanCode,
  dedupeFindings,
  FIXTURE_SIGNALS,
  RESPONSIBILITY_STAGES,
};
