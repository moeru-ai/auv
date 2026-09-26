import process from 'node:process'

import * as fs from 'node:fs'
import * as path from 'node:path'

import { app, BrowserWindow, Menu } from 'electron'

const [url, profile, output] = process.argv.slice(2)

app.setPath('userData', profile)
app.setName('AUV Keyboard Electron Receiver')

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

  // A second independent observation point, before renderer/DOM dispatch.
  window.webContents.on('before-input-event', (_event, input) => {
    fs.appendFileSync(path.join(output, 'electron-native-events.jsonl'), `${JSON.stringify({ time: Date.now(), ...input })}\n`)
  })

  await window.loadURL(url)
})

app.on('window-all-closed', () => app.quit())

setTimeout(() => app.quit(), 300_000).unref()
