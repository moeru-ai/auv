// An app that owns a daemon and dies without stopping it. It prints the
// daemon pid, then idles until the test kills it.
import process from 'node:process'

import { startAuv } from '../node/daemon'

const [binaryPath, workingDirectory] = process.argv.slice(2)
void startAuv({ binaryPath, noRegister: true, workingDirectory }).then((daemon) => {
  process.stdout.write(`${daemon.pid}\n`)
  setInterval(() => {}, 60_000)
})
