const { app, BrowserWindow, Menu } = require('electron');
const fs = require('node:fs');
const path = require('node:path');

const [url, profile, output] = process.argv.slice(2);
app.setPath('userData', profile);
app.setName('AUV Keyboard Electron Receiver');
app.whenReady().then(async () => {
  fs.writeFileSync(path.join(output, 'electron-versions.json'), JSON.stringify(process.versions, null, 2));
  Menu.setApplicationMenu(Menu.buildFromTemplate([
    { role: 'appMenu' },
    { role: 'editMenu' },
  ]));
  const window = new BrowserWindow({
    width: 640, height: 420,
    webPreferences: { nodeIntegration: false, contextIsolation: true, sandbox: true },
  });
  // A second independent observation point, before renderer/DOM dispatch.
  window.webContents.on('before-input-event', (_event, input) => {
    fs.appendFileSync(path.join(output, 'electron-native-events.jsonl'), JSON.stringify({ time: Date.now(), ...input }) + '\n');
  });
  await window.loadURL(url);
});
app.on('window-all-closed', () => app.quit());
setTimeout(() => app.quit(), 300_000).unref();
