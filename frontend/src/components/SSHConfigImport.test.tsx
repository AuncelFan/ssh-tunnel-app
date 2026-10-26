// @vitest-environment jsdom
import React from 'react'
import { afterEach, beforeAll, expect, test, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { SSHConfigImport } from './SSHConfigImport'
import { api } from '../api'
import { newJumper } from '../defaults'

vi.mock('../api', () => ({ api: { defaultSSHConfigPath: vi.fn(), loadSSHConfig: vi.fn() } }))
beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', '') }
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open') }
})

test('path input waits for the default path to avoid inserting a default during user entry', async () => {
  let finish!: (path: string) => void
  vi.mocked(api.defaultSSHConfigPath).mockReturnValue(new Promise(resolve => { finish = resolve }))
  render(<SSHConfigImport jumpers={[]} save={vi.fn()} saveMany={vi.fn()} close={vi.fn()} />)
  const field = screen.getByLabelText('SSH config path') as HTMLInputElement
  expect(field.disabled).toBe(true)
  expect((screen.getByRole('button', { name: 'Load aliases' }) as HTMLButtonElement).disabled).toBe(true)
  await act(async () => { finish('C:\\default\\config') })
  expect(field.disabled).toBe(false)
  fireEvent.change(field, { target: { value: 'C:\\custom\\config' } })
  expect(field.value).toBe('C:\\custom\\config')
})

test('bulk selection excludes existing and incomplete aliases and requires confirmation', async () => {
  vi.mocked(api.defaultSSHConfigPath).mockResolvedValue('C:\\fixture\\config')
  const existing = { ...newJumper(), id: 1, name: 'Existing', host: 'existing.test', user: 'fixture', auth_type: 'ssh_agent' as const }
  const fresh = { ...existing, id: 0, name: 'Fresh', host: 'fresh.test' }
  vi.mocked(api.loadSSHConfig).mockResolvedValue({ candidates: [
    { alias: 'Existing', jumper: existing, proxy_jump: '', warnings: [] },
    { alias: 'Incomplete', jumper: { ...fresh, user: '' }, proxy_jump: '', warnings: ['missing_user'] },
    { alias: 'Fresh', jumper: fresh, proxy_jump: '', warnings: [] },
  ], warnings: [] })
  const saveMany = vi.fn().mockResolvedValue({ imported: [2], skipped: 0 })
  render(<SSHConfigImport jumpers={[existing]} save={vi.fn()} saveMany={saveMany} close={vi.fn()} />)
  await waitFor(() => expect((screen.getByLabelText('SSH config path') as HTMLInputElement).value).toContain('fixture'))
  fireEvent.click(screen.getByRole('button', { name: 'Load aliases' }))
  fireEvent.click(await screen.findByRole('button', { name: 'Select all' }))
  expect((screen.getByRole('checkbox', { name: 'Existing' }) as HTMLInputElement).disabled).toBe(true)
  expect((screen.getByRole('checkbox', { name: 'Incomplete' }) as HTMLInputElement).disabled).toBe(true)
  expect((screen.getByRole('checkbox', { name: 'Fresh' }) as HTMLInputElement).checked).toBe(true)
  fireEvent.click(screen.getByRole('button', { name: 'Import selected (1)' }))
  expect(saveMany).not.toHaveBeenCalled()
  fireEvent.click(within(screen.getByRole('dialog', { name: 'Import selected aliases?' })).getByRole('button', { name: 'Import selected' }))
  await waitFor(() => expect(saveMany).toHaveBeenCalledWith([fresh]))
  await screen.findByText('Imported: 1; Duplicates skipped: 0')
})
afterEach(() => { cleanup(); vi.resetAllMocks() })
test('loading only previews aliases, requires review, and clears stale results on failure', async () => {
  vi.mocked(api.defaultSSHConfigPath).mockResolvedValue('C:\\fixture\\config')
  vi.mocked(api.loadSSHConfig).mockResolvedValueOnce({ candidates: [{ alias: 'server', jumper: { ...newJumper(), name: 'server', host: 'example.test', user: 'fixture', auth_type: 'ssh_agent' }, proxy_jump: 'gateway', warnings: ['proxy_jump'] }], warnings: [] }).mockRejectedValueOnce(new Error('Cannot read SSH config'))
  const save = vi.fn().mockResolvedValue(undefined)
  render(<SSHConfigImport saveMany={vi.fn()} jumpers={[]} save={save} close={vi.fn()} />)
  await waitFor(() => expect((screen.getByLabelText('SSH config path') as HTMLInputElement).value).toBe('C:\\fixture\\config'))
  fireEvent.click(screen.getByRole('button', { name: 'Load aliases' }))
  await screen.findByRole('button', { name: 'Review and import' })
  expect(save).not.toHaveBeenCalled()
  expect(screen.getByText('ProxyJump: gateway')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Load aliases' }))
  await screen.findByRole('alert')
  expect(screen.queryByRole('button', { name: 'Review and import' })).toBeNull()
})
