# List available task groups.
default:
    @just --list

# Build, check, and run AUV base evaluations.
mod eval 'evals/auv-base/justfile'
