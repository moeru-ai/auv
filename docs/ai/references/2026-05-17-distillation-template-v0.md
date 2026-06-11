# AUV Distillation Template v0

Date: 2026-05-17

Status: working template

## Purpose

This template captures the minimum useful shape for ongoing skill distillation.

It is not the final artifact schema. It is the smallest repeatable document
shape that fits the current QQ音乐, Notes, and TextEdit evidence.

## Inputs

Collect these before asking a model to distill:

- validated recipe manifest
- validated case matrix
- live run ids
- artifact paths
- failure notes
- current app-specific limits
- reusable contract candidates

## Distillation Prompt Shape

Use a prompt that asks for only proven facts:

1. identify the validated action chain
2. identify the reusable contract
3. identify the required commands
4. identify the exact inputs
5. identify the known limits
6. do not invent missing coverage
7. do not widen scope to generalized product claims

## Output Shape

The distilled output should answer:

- what app or app family is covered
- what the executable entrypoint is
- what the verified chain is
- what the validation contract is
- what the known limits are
- what still needs live replay

## What To Preserve

- exact command ids
- exact step ordering
- exact scope of verification
- exact failure boundaries
- exact app-specific caveats

## What Not To Preserve

- speculative generalized product claims
- unsupported broad reuse claims
- unverified control surfaces
- unvalidated OCR assumptions
- invented fallback strategies

## Current Working Sample Set

- QQ音乐 narrow playback slice
- Notes AX text sample
- TextEdit AX text sample

These three samples are enough to drive the next round of controlled
distillation without starting from raw screenshots or free-form chat logs.

The first bundle-shaped container for those samples was retired on 2026-06-11:

- historical `native-app-skill-tree` manifest

## Practical Next Step

The next distillation run should compare:

- QQ音乐 playback skill
- Notes AX text sample
- TextEdit AX text sample

and ask whether the reusable center is:

- app-specific search/playback
- generic AX text verification
- or a bundle that contains both as separate strategies
