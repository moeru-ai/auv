import { describe, expect, it } from "vitest";

import auvPlugin from "../../index";
import { noSourceFilesCompareInTestsPrompt } from "./prompt";
import { noSourceFilesCompareInTestsRule } from "./rule";

describe("no-source-files-compare-in-tests", () => {
  it("is registered by the AUV plugin", () => {
    expect(auvPlugin.rules?.["no-source-files-compare-in-tests"]).toBe(noSourceFilesCompareInTestsRule);
  });

  it("describes source-layout checks and the behavioral replacement", () => {
    expect(noSourceFilesCompareInTestsPrompt).toContain("checks whether an implementation source file exists");
    expect(noSourceFilesCompareInTestsPrompt).toContain("compares discovered source files");
    expect(noSourceFilesCompareInTestsPrompt).toContain("TypeScript");
    expect(noSourceFilesCompareInTestsPrompt).toContain("public, typed, observable behavior");
    expect(noSourceFilesCompareInTestsPrompt).toContain("meaningless as behavioral evidence");
  });

  it("excludes fixtures and observable filesystem behavior", () => {
    expect(noSourceFilesCompareInTestsPrompt).toContain("JSON, images, recorded artifacts");
    expect(noSourceFilesCompareInTestsPrompt).toContain("public filesystem API");
  });
});
