# Sandbox Bridge

This workspace package connects the AUV JavaScript SDK to a Runner from QuickJS.
The host passes the selected Runner and a recording callback, then registers
the returned globals with the sandbox.

```ts
import { createSandboxRunnerHost } from '@auv-js/sandbox-bridge'

const host = createSandboxRunnerHost(access, signal, record)
try {
  await runSandbox(host.globals)
}
finally {
  await host.close()
}
```

Inside the sandbox, load the runtime's encoding and Headers polyfills. The
Playground entry installs timers explicitly before creating the SDK client.

```ts
import { createSandboxRunner, installSandboxTimers } from '@auv-js/sandbox-bridge'

installSandboxTimers()
export const auv = createSandboxRunner()
```
