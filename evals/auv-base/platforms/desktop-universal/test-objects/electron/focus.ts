import process from 'node:process'

import * as fs from 'node:fs'
import * as path from 'node:path'

import { app, BrowserWindow, Menu } from 'electron'

const [url, profile, output] = process.argv.slice(2)

app.setPath('userData', profile)
app.setName('AUV No Raise Receiver')

app.whenReady().then(async () => {
  Menu.setApplicationMenu(Menu.buildFromTemplate([
    { role: 'appMenu' },
    { role: 'editMenu' },
  ]))

  const window = new BrowserWindow({
    height: 420,
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
    width: 640,
  })

  window.webContents.on('before-input-event', (_event, input) => {
    fs.appendFileSync(path.join(output, 'native-events.jsonl'), `${JSON.stringify({ timeMs: Date.now(), ...input })}\n`)
  })

  let previous: string | undefined

  setInterval(() => {
    if (window.isDestroyed())
      return

    const value = {
      contentsFocused: window.webContents.isFocused(),
      windowFocused: window.isFocused(),
    }
    const encoded = JSON.stringify(value)

    if (encoded !== previous) {
      fs.appendFileSync(path.join(output, 'native-state.jsonl'), `${JSON.stringify({ timeMs: Date.now(), ...value })}\n`)
      previous = encoded
    }
  }, 10)

  await window.loadURL(url)
})

app.on('window-all-closed', () => app.quit())

setTimeout(() => app.quit(), 900_000).unref()
