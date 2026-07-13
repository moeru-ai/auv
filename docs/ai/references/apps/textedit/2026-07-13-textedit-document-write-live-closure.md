# TextEdit document.write live macOS closure

**Status:** blocked / opt-in (not claimed as product-supported until this passes)

## Manual command

```sh
# TextEdit must be open with a document window; Accessibility granted to the terminal host.
AUV_TEXTEDIT_LIVE=1 cargo test -p auv-product --test textedit_document_write_parity \
  textedit_document_write_live_macos_closure -- --ignored --nocapture
```

Or product invoke:

```sh
auv invoke app.textedit.document.write --content 'AUV_TEXTEDIT_LIVE_MARKER' --verify true
auv inspect <run_id>
```

## Must prove

1. TextEdit document body focused via typed AX API
2. Requested text delivered (clipboard paste)
3. AX text observation reads resulting body
4. Semantic `VerificationResult` method=`ax_text` matched
5. One canonical run inspectable via product CLI / MCP / inspect-server

## CI

Hermetic path uses `--driver fixture` (see `textedit_document_write_same_run_cli_mcp_inspect_parity`).
