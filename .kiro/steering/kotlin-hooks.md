---
inclusion: fileMatch
fileMatchPattern: ['**/*.kt', '**/*.kts', '**/build.gradle.kts']
---
# Kotlin Hooks

> This file extends the common hooks rule with Kotlin-specific content.

## PostToolUse Hooks

Configure in `~/.claude/settings.json`:

- **ktfmt/ktlint**: Auto-format `.kt` and `.kts` files after edit
- **detekt**: Run static analysis after editing Kotlin files
- **./gradlew build**: Verify compilation after changes
