# TextEdit document.write live macOS closure

**Status:** blocked / opt-in (not claimed as product-supported until this passes)

## Manual command

```sh
# TextEdit must be open with a document window; Accessibility granted to the terminal host.
AUV_TEXTEDIT_LIVE=1 \
cargo test -p auv-cli \
  --test textedit_document_write_parity \
  textedit_document_write_live_macos_closure -- --ignored --nocapture
```

Or product invoke:

```sh
cargo run -p auv-cli -- \
  invoke app.textedit.document.write \
  --content 'AUV_TEXTEDIT_LIVE_MARKER' \
  --verify true
cargo run -p auv-cli -- inspect <run_id>
```

## Must prove

1. TextEdit document body focused via typed AX API
2. Requested text delivered (clipboard paste)
3. AX text observation reads resulting body
4. Semantic `VerificationResult` method=`ax_text` has `semantic_matched=true`
5. One canonical run is persisted and inspectable with the product CLI

## Evidence boundary

`semantic_matched=true` means the post-write AX text contains the requested
content. This operation does not capture a pre-write AX observation, so it
intentionally records `VerificationResult.state_changed=false`; it cannot prove
a state transition until pre/post evidence is added. MCP and inspect-server
same-run parity remains covered by the hermetic fixture test, not this live
closure.

## CI

Hermetic path uses `--driver fixture` (see `textedit_document_write_same_run_cli_mcp_inspect_parity`).
