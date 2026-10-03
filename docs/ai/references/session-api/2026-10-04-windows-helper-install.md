# Windows Helper installation lifecycle

Status: implemented for review; Windows native compilation and a clean-host
installation gate must pass before this becomes a release support claim.

`auv setup windows-helper` owns the installed lifecycle for the Windows Device
entry host shipped in the release ZIP. The public product name is **AUV
Helper**. Its installation consists of both `auv.exe`, which hosts the
`AuvDevice` SCM service, and the sibling one-shot `auv-helper.exe` worker.

## Commands and fixed layout

Run these commands from an elevated PowerShell after extracting the Windows
release ZIP:

```powershell
.\auv.exe setup windows-helper status
.\auv.exe setup windows-helper install
.\auv.exe setup windows-helper uninstall
```

Installation creates a protected `%ProgramFiles%\AUV` directory with a
non-inheriting Administrators-and-SYSTEM-only DACL, copies both release
binaries, registers `AuvDevice` as an automatic LocalSystem service, and waits
for it to report `Running`. The service command continues to use the reviewed
fixed contract: one `http://127.0.0.1:9847` listener and the SYSTEM-only
`%ProgramData%\AUVDeviceEntry` store and `pairings.json` path.

The installer refuses a preexisting installation directory or service instead
of adopting files or configuration it did not create. `status` reports
`ready`, `degraded`, or `not_installed` and treats a service with a different
binary, arguments, account, or startup type as foreign. `uninstall` performs
the same ownership check before stopping or deleting the service and refuses
to remove an installation directory containing unknown entries. It preserves
the durable Device pairing, policy, audit, and credential store. Destructive
purge remains intentionally deferred pending an owner-approved confirmation
contract.

## First pairing without command-line secrets

Clean-host installation also creates a temporary `AuvDeviceBootstrap` service.
That process runs as LocalSystem in Session 0, creates the protected Device
store, and writes one twenty-minute bootstrap token to
`%ProgramData%\AUVBootstrap\pairing-token.txt`. The token is never placed in a
process command line, environment variable, service definition, trace, or
installer output. The temporary service is deleted before `AuvDevice` starts.

The paired listener remains loopback-only. From the client Mac, use the
existing SSH connection as the transport and feed the token through stdin so
it is not copied into shell history:

```bash
ssh -N -L 9847:127.0.0.1:9847 luoling-windows-11
ssh luoling-windows-11 'powershell -NoProfile -Command "Get-Content -Raw $env:ProgramData\AUVBootstrap\pairing-token.txt"' \
  | auv devices pair --endpoint http://127.0.0.1:9847 connect \
      --token-stdin --label 'luoling-windows-11' --profile luoling-windows-11
ssh luoling-windows-11 'powershell -NoProfile -Command "& \"$env:ProgramFiles\AUV\auv.exe\" setup windows-helper clear-bootstrap-token"'
```

The final command deletes the consumed target-local token file and its empty
bootstrap directory. If pairing fails, leave the file in place only long
enough to retry within its expiry, then clear it.

## Evidence boundary

Compilation proves only that the Windows-specific SCM and security APIs are
well formed. The installation gate must additionally verify the effective
directory DACL, LocalSystem/Session 0 service identity, automatic startup,
loopback listener, target-local PIN enrollment, locked-session worker
placement, semantic unlock verification, cleanup, and the absence of secrets
from command lines and captured output. Authenticode signing remains the
separate release-policy TODO recorded in the Windows helper release note.
