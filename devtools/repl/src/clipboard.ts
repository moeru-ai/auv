/** Copies text to the clipboard. */
export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text)
  }
  catch {
    // NOTICE(clipboard-fallback): the async Clipboard API needs a secure
    // context and permission; fall back to the legacy selection copy.
    const area = document.createElement('textarea')
    area.value = text
    document.body.append(area)
    area.select()
    document.execCommand('copy')
    area.remove()
  }
}
