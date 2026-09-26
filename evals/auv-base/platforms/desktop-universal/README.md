# Shared desktop test objects

`test-objects/electron/` and `test-objects/web/` contain TypeScript receivers used
by desktop tasks. Run `just eval build` from the repository root to build them
along with the evaluation runners.

The directory names identify application technologies. Current modules observe
keyboard input and focus; future approved evaluations can add other test areas
to these applications. See [test object scope and naming](../../README.md#test-object-scope-and-naming).

Native platform setup and platform-specific assertions belong under the matching
platform's `tasks/` and `cases/`. NOTICE: These receivers have currently been
validated only by macOS tasks; Windows/Linux behavior requires its own evaluation.
