// An app that owns a daemon and dies without stopping it. It prints the
// daemon pid and owner endpoint, then idles until the test kills it.
import process from 'node:process'

import { startAuv } from '../node/daemon'

const [binaryPath, workingDirectory] = process.argv.slice(2)
void startAuv({ binaryPath, noRegister: true, workingDirectory }).then((daemon) => {
  process.stdout.write(`${daemon.pid} ${daemon.connectionOptions.endpoint}\n`)
  setInterval(() => {}, 60_000)
})
