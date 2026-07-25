import { describe, expect, it } from "vitest";

import auvPlugin from "../../index";
import { noModNamesChecksInTestsPrompt } from "./prompt";
import { noModNamesChecksInTestsRule } from "./rule";

describe("no-mod-names-checks-in-tests", () => {
  it("is registered by the AUV plugin", () => {
    expect(auvPlugin.rules?.["no-mod-names-checks-in-tests"]).toBe(noModNamesChecksInTestsRule);
  });

  it("describes declaration-name checks and why they are invalid", () => {
    expect(noModNamesChecksInTestsPrompt).toContain("module declarations or exports");
    expect(noModNamesChecksInTestsPrompt).toContain("struct, enum, trait, type alias");
    expect(noModNamesChecksInTestsPrompt).toContain("do not prove behavior");
    expect(noModNamesChecksInTestsPrompt).toContain("removed entirely");
  });

  it("allows typed behavior and compiler-enforced API use", () => {
    expect(noModNamesChecksInTestsPrompt).toContain("ordinary assertions over typed values");
    expect(noModNamesChecksInTestsPrompt).toContain("compiler-enforced imports");
  });
});
