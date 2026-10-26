// @vitest-environment jsdom
import React from 'react'
import { afterEach, beforeAll, expect, test, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { CommandImport } from './CommandImport'
import { api } from '../api'
import { newJumper, newTunnel } from '../defaults'

vi.mock('../api', () => ({ api: { parseSSHCommand: vi.fn() } }))
beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', '') }
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open') }
})
afterEach(() => { cleanup(); vi.resetAllMocks() })
test('parsing is read-only, reuses saved credentials and imports only after confirmation', async () => {
  const jumper = { ...newJumper(), id: 7, host: 'example.test', user: 'fixture', auth_type: 'password' as const, password: 'fixture-only' }
  const tunnel = { ...newTunnel(), name: 'Service', local_port: 8080, remote_port: 80 }
  vi.mocked(api.parseSSHCommand).mockResolvedValue({ jumpers: [{ ...jumper, id: 0, password: '', auth_type: 'ssh_agent' }], tunnels: [tunnel], warnings: [] })
  const save = vi.fn().mockResolvedValue({ imported: [1], created_jumpers: [], skipped: 0 })
  render(<CommandImport jumpers={[jumper]} tunnels={[]} close={vi.fn()} save={save} />)
  fireEvent.change(screen.getByLabelText('SSH command'), { target: { value: 'ssh -L8080:host:80 fixture@example.test' } })
  fireEvent.click(screen.getByRole('button', { name: 'Parse command' }))
  fireEvent.click(await screen.findByRole('button', { name: 'Import selected (1)' }))
  expect(save).not.toHaveBeenCalled()
  fireEvent.click(within(screen.getByRole('dialog', { name: 'Import selected tunnels?' })).getByRole('button', { name: 'Import selected' }))
  await waitFor(() => expect(save).toHaveBeenCalledWith({ jumpers: [jumper], tunnels: [tunnel] }))
})
test('duplicate mappings cannot be selected for import', async () => {
  const tunnel = { ...newTunnel(), name: 'Service', local_port: 8080, remote_port: 80 }
  vi.mocked(api.parseSSHCommand).mockResolvedValue({ jumpers: [{ ...newJumper(), user: 'fixture' }], tunnels: [tunnel], warnings: [] })
  render(<CommandImport jumpers={[]} tunnels={[tunnel]} close={vi.fn()} save={vi.fn()} />)
  fireEvent.change(screen.getByLabelText('SSH command'), { target: { value: 'ssh -L8080:host:80 fixture@example.test' } })
  fireEvent.click(screen.getByRole('button', { name: 'Parse command' }))
  await screen.findByText('Duplicate mapping')
  expect((screen.getByRole('button', { name: 'Import selected (0)' }) as HTMLButtonElement).disabled).toBe(true)
})
