import AppKit
import Carbon

// The fixture controls and restores foreground/input-source state around its
// own windows. Physical-key expectations use ABC, not a composing input method.
if CommandLine.arguments.dropFirst().first == "source" {
  if CommandLine.arguments.count > 2 {
    let filter = [kTISPropertyInputSourceID: CommandLine.arguments[2]] as CFDictionary
    let sources = TISCreateInputSourceList(filter, false).takeRetainedValue() as! [TISInputSource]
    guard let source = sources.first, TISSelectInputSource(source) == noErr else {
      exit(1)
    }
  }

  let source = TISCopyCurrentKeyboardInputSource().takeRetainedValue()
  guard let property = TISGetInputSourceProperty(source, kTISPropertyInputSourceID) else {
    exit(1)
  }

  print(Unmanaged<CFString>.fromOpaque(property).takeUnretainedValue() as String)
} else if CommandLine.arguments.count > 1, let pid = Int32(CommandLine.arguments[1]) {
  print(NSRunningApplication(processIdentifier: pid)?.activate(options: []) ?? false)
} else {
  print(NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0)
}
