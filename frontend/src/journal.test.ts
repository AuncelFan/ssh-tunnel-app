// @vitest-environment jsdom
import { afterEach, expect, test, vi } from 'vitest'
import { copyText, duplicateJumper, logLevel, logMessage, logTime } from './journal'
import { newJumper } from './defaults'
import type { RuntimeEvent } from './types'

afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); document.body.replaceChildren() })
const entry: RuntimeEvent = { tunnel_id: 1, generation: 1, timestamp_ms: 1700000000000, kind: 'status', message: 'reconnecting', challenge: null }
test('duplicate jumper preserves connection settings but does not edit or reuse the original ID', () => {
  const original = Object.freeze({ ...newJumper(), id: 9, name: 'Gateway', password: 'fixture-secret', notes: 'Preserved notes' })
  const copy = duplicateJumper(original, 'copy')
  expect(copy).toEqual({ ...original, id: 0, name: 'Gateway copy' })
  copy.host = 'another.invalid'
  expect(original.host).not.toBe(copy.host)
})
test('journal distinguishes retry warnings, transport errors and configuration activity', () => {
  expect(logLevel(entry)).toBe('warn')
  expect(logLevel({ ...entry, kind: 'host_key' })).toBe('warn')
  expect(logLevel({ ...entry, kind: 'action_error' })).toBe('error')
  expect(logMessage({ ...entry, kind: 'action', message: 'save_jumper' }, key => key)).toBe('Save jumper: Succeeded')
  expect(logTime(entry, 'en')).toMatch(/\d{2}:\d{2}/)
  expect(logTime({ ...entry, timestamp_ms: undefined }, 'en')).toBe('--')
})
test('clipboard API copies exactly the error text', async () => {
  const writeText = vi.fn().mockResolvedValue(undefined)
  vi.stubGlobal('navigator', { clipboard: { writeText } })
  await copyText('Synthetic error\nDetails')
  expect(writeText).toHaveBeenCalledWith('Synthetic error\nDetails')
})
test('clipboard fallback stays inside a dialog and restores focus without leaving temporary content', async () => {
  vi.stubGlobal('navigator', { clipboard: { writeText: vi.fn().mockRejectedValue(new Error('Denied')) } })
  const dialog = document.createElement('dialog')
  dialog.open = true
  const button = document.createElement('button')
  dialog.appendChild(button); document.body.appendChild(dialog); button.focus()
  Object.defineProperty(document, 'execCommand', { configurable: true, value: vi.fn(() => {
    expect(dialog.querySelector('textarea')?.value).toBe('Synthetic error')
    return true
  }) })
  try { await copyText('Synthetic error'); expect(document.querySelector('textarea')).toBeNull(); expect(document.activeElement).toBe(button) }
  finally { Reflect.deleteProperty(document, 'execCommand') }
})
test('clipboard failure is reported rather than claiming a copy succeeded', async () => {
  vi.stubGlobal('navigator', {})
  await expect(copyText('Synthetic error')).rejects.toThrow('Clipboard unavailable')
  expect(document.querySelector('textarea')).toBeNull()
})
