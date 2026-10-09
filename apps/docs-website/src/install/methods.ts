// Install methods per system, mirrored from the "Getting Started" section of
// the AUV README (github.com/moeru-ai/auv). Keep the commands in sync with it.
//
// `helper` says whether the method ships the system helper (signed
// `AUV Helper.app` on macOS, `auv-helper.exe` embedded in `auv.exe` on
// Windows); only those methods show the helper setup step.

import type { IconName } from '../lib/icons'

export interface Arch { id: string, label: string }
export interface Method {
  archs?: Arch[]
  /** Commands, one per line; `#` lines are comments. */
  code: ((arch: string) => string) | string
  helper?: boolean
  icon: IconName
  id: string
  label: string
  note?: string
}
export interface Os {
  icon: IconName
  id: OsId
  label: string
  methods: Method[]
  /** The helper step: what the helper is for, then how to install it. */
  setup?: { about: string, code: string, note: string, title: string }
  shell: Shell
}

export type OsId = 'linux' | 'macos' | 'windows'

export type Shell = 'powershell' | 'sh'

const REPO = 'https://github.com/moeru-ai/auv'
const SCRIPT = 'curl -fsSL https://raw.githubusercontent.com/moeru-ai/auv/main/install/install.sh | sh'
const SCRIPT_NOTE = 'Installs to ~/.local/bin. Set AUV_VERSION or AUV_INSTALL_DIR to change the version or the directory.'
const PROTO = [
  `proto plugin add auv "https://raw.githubusercontent.com/moeru-ai/auv/main/toolchain/proto/auv.toml" --to global`,
  'proto install auv latest --config-mode global --pin global',
].join('\n')
const PROTO_NOTE = 'Needs proto (moonrepo.dev/proto).'
const CARGO = `cargo install --git ${REPO} auv-cli --bin auv`
const NIX = `nix profile install 'git+${REPO}#default'`
const VERSION = 'auv --version'

export const SYSTEMS: Os[] = [
  {
    icon: 'apple-logo',
    id: 'macos',
    label: 'macOS',
    methods: [
      { code: `brew install moeru-ai/tap/auv\n${VERSION}`, helper: true, icon: 'beer-stein', id: 'brew', label: 'Homebrew' },
      { code: `${SCRIPT}\n${VERSION}`, icon: 'terminal-window', id: 'script', label: 'Script', note: `${SCRIPT_NOTE} The binary only: no AUV Helper.` },
      { code: `${PROTO}\n${VERSION}`, helper: true, icon: 'wrench', id: 'proto', label: 'proto', note: PROTO_NOTE },
      { code: `xcode-select --install\n${NIX}\n${VERSION}`, icon: 'snowflake', id: 'nix', label: 'Nix', note: 'Needs Nix 2.27+ with nix-command and flakes. Builds from source, without AUV Helper.' },
      { code: `xcode-select --install\n${CARGO}\n${VERSION}`, icon: 'package', id: 'cargo', label: 'Cargo', note: 'Needs Rust. Builds from source, without AUV Helper.' },
    ],
    setup: {
      // From crates/auv-device-helper-macos/README.md in the AUV repository.
      about: 'AUV Helper is a small signed app that runs in the background of your user session. It lets AUV unlock the screen after it locks, so long runs keep going. The unlock credential stays in your login Keychain, and only the helper reads it.',
      code: 'auv setup macos-helper install\nauv setup macos-helper status',
      note: 'macOS 13 or later, no sudo. AUV itself also needs Accessibility and Screen Recording for your terminal: grant them, restart the terminal, then run auv doctor.',
      title: 'Then install AUV Helper',
    },
    shell: 'sh',
  },
  {
    icon: 'linux-logo',
    id: 'linux',
    label: 'Linux',
    methods: [
      { code: `${SCRIPT}\n${VERSION}`, icon: 'terminal-window', id: 'script', label: 'Script', note: `${SCRIPT_NOTE} glibc only; musl is not supported.` },
      { code: `${PROTO}\n${VERSION}`, icon: 'wrench', id: 'proto', label: 'proto', note: PROTO_NOTE },
      { code: `${NIX}\n${VERSION}`, icon: 'snowflake', id: 'nix', label: 'Nix', note: 'Needs Nix 2.27+ with nix-command and flakes. Builds from source for x86-64 and ARM64.' },
      {
        code: [
          '# Ubuntu / Debian build dependencies',
          'sudo apt-get update',
          'sudo apt-get install -y pkg-config libclang-dev libxcb1-dev libxrandr-dev libdbus-1-dev libpipewire-0.3-dev libwayland-dev libxkbcommon-dev libegl-dev libleptonica-dev libtesseract-dev',
          CARGO,
          VERSION,
        ].join('\n'),
        icon: 'package',
        id: 'cargo',
        label: 'Cargo',
        note: 'Needs Rust. Other distributions can use different package names.',
      },
    ],
    shell: 'sh',
  },
  {
    icon: 'windows-logo',
    id: 'windows',
    label: 'Windows',
    methods: [
      { code: `scoop bucket add auv ${REPO}\nscoop install auv/auv\n${VERSION}`, helper: true, icon: 'ice-cream', id: 'scoop', label: 'Scoop' },
      {
        archs: [{ id: 'x86_64', label: 'x86-64' }, { id: 'aarch64', label: 'ARM64' }],
        code: arch => [
          `Invoke-WebRequest ${REPO}/releases/latest/download/auv-${arch}-pc-windows-msvc.zip -OutFile auv.zip`,
          `Expand-Archive auv.zip "$env:LOCALAPPDATA\\Programs\\auv"`,
          '# Add it to your user PATH, then open a new terminal',
          `[Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ";$env:LOCALAPPDATA\\Programs\\auv", 'User')`,
        ].join('\n'),
        helper: true,
        icon: 'download-simple',
        id: 'zip',
        label: 'Download',
        note: 'The archive holds a single auv.exe with the Windows helper embedded.',
      },
      { code: `${PROTO}\n${VERSION}`, helper: true, icon: 'wrench', id: 'proto', label: 'proto', note: PROTO_NOTE },
      { code: `${CARGO}\n${VERSION}`, icon: 'package', id: 'cargo', label: 'Cargo', note: 'Needs Rust (MSVC toolchain), Visual Studio Build Tools, and the Windows SDK. Without the helper.' },
    ],
    setup: {
      about: 'The helper is a Windows service (AuvHelper) that runs as LocalSystem, so AUV can lock and unlock the session it works in. Setup copies it out of auv.exe into %ProgramFiles%\\AUV. It does not listen on the network.',
      code: 'auv setup windows-helper install\nauv setup windows-helper status',
      note: 'Run these in an elevated PowerShell.',
      title: 'Then install the helper service',
    },
    shell: 'powershell',
  },
]

export function codeOf(m: Method, arch: string) {
  return typeof m.code === 'function' ? m.code(arch) : m.code
}

/** Best guess at the visitor's system and CPU; phones fall back to macOS. */
export function detectSystem(): { arch: string, os: OsId } {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } }
  const p = `${nav.userAgentData?.platform ?? ''} ${navigator.platform} ${navigator.userAgent}`.toLowerCase()
  const os: OsId = p.includes('win') ? 'windows' : p.includes('linux') && !p.includes('android') ? 'linux' : 'macos'
  return { arch: /arm|aarch64/.test(p) ? 'aarch64' : 'x86_64', os }
}
