// @vitest-environment jsdom
import React from 'react'
import { afterEach, beforeAll, beforeEach, expect, test, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { api, type ProbeReport } from '../api'
import { newJumper } from '../defaults'
import { ProbeActions } from './ProbeActions'

vi.mock('../api', () => ({ api: { beginProbe: vi.fn(), runProbe: vi.fn(), approveProbeKey: vi.fn(), cancelProbe: vi.fn() } }))
const passed: ProbeReport = { success: true, elapsed_ms: 12, latency_ms: 1.5, checks: [{ phase: 'latency', ok: true, code: 'ok', detail: '' }], challenge: null }
beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', '') }
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open') }
})
beforeEach(() => {
  vi.mocked(api.beginProbe).mockResolvedValue(42)
  vi.mocked(api.runProbe).mockResolvedValue(passed)
  vi.mocked(api.cancelProbe).mockResolvedValue(undefined)
  vi.mocked(api.approveProbeKey).mockResolvedValue(undefined)
})
afterEach(() => { cleanup(); vi.resetAllMocks() })

test('diagnostics test the supplied draft and show measured latency', async () => {
  const jumper = newJumper()
  render(<ProbeActions jumpers={[jumper]} />)
  fireEvent.click(screen.getByRole('button', { name: 'Diagnose connection' }))
  await screen.findByText('Connection test passed')
  expect(api.beginProbe).toHaveBeenCalledWith({ jumpers: [jumper], tunnel: null, diagnostics: true })
  expect(api.runProbe).toHaveBeenCalledWith(42)
  expect(screen.getByText(/1.50 ms/)).toBeTruthy()
  fireEvent.click(within(screen.getByRole('dialog')).getAllByRole('button', { name: 'Close test' }).at(-1)!)
  await waitFor(() => expect(api.cancelProbe).toHaveBeenCalledWith(42))
})

test('cancelling before the native token arrives never starts networking', async () => {
  let finish!: (token: number) => void
  vi.mocked(api.beginProbe).mockReturnValue(new Promise(resolve => { finish = resolve }))
  render(<ProbeActions jumpers={[newJumper()]} />)
  fireEvent.click(screen.getByRole('button', { name: 'Test connection' }))
  fireEvent.click(await screen.findByRole('button', { name: 'Cancel test' }))
  await act(async () => { finish(43) })
  await waitFor(() => expect(api.cancelProbe).toHaveBeenCalledWith(43))
  expect(api.runProbe).not.toHaveBeenCalled()
})

test('host-key confirmation approves only the native token then reruns the test', async () => {
  vi.mocked(api.runProbe).mockResolvedValueOnce({ ...passed, success: false, challenge: { jumper_id: 0, host: 'example.invalid', port: 22, previous_fingerprint: null, fingerprint: 'SHA256:fixture', public_key: 'fixture' } }).mockResolvedValueOnce(passed)
  render(<ProbeActions jumpers={[newJumper()]} />)
  fireEvent.click(screen.getByRole('button', { name: 'Test connection' }))
  const dialog = await screen.findByRole('dialog', { name: 'Trust this server?' })
  expect(api.approveProbeKey).not.toHaveBeenCalled()
  fireEvent.click(within(dialog).getByRole('button', { name: 'Trust key and connect' }))
  await screen.findByText('Connection test passed')
  expect(api.approveProbeKey).toHaveBeenCalledWith(42)
  expect(api.runProbe).toHaveBeenCalledTimes(2)
})

test('unmount cancels an in-flight test and ignores a late result', async () => {
  let finish!: (report: ProbeReport) => void
  vi.mocked(api.runProbe).mockReturnValue(new Promise(resolve => { finish = resolve }))
  const view = render(<ProbeActions jumpers={[newJumper()]} />)
  fireEvent.click(screen.getByRole('button', { name: 'Test connection' }))
  await waitFor(() => expect(api.runProbe).toHaveBeenCalledWith(42))
  view.unmount()
  expect(api.cancelProbe).toHaveBeenCalledWith(42)
  await act(async () => { finish(passed) })
  expect(screen.queryByRole('dialog')).toBeNull()
})
