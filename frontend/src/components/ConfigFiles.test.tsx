// @vitest-environment jsdom
import React from 'react'
import { afterEach, beforeAll, beforeEach, expect, test, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { api } from '../api'
import { ConfigFiles } from './ConfigFiles'

vi.mock('../api', () => ({ api: { configPath: vi.fn(), chooseConfigFile: vi.fn(), previewImport: vi.fn(), confirmImport: vi.fn(), cancelImport: vi.fn(), exportConfig: vi.fn() } }))
beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', '') }
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open') }
})
beforeEach(() => { vi.mocked(api.configPath).mockResolvedValue('C:\\fixture\\config.toml'); vi.mocked(api.cancelImport).mockResolvedValue(undefined) })
afterEach(() => { cleanup(); vi.resetAllMocks() })

test('import requires explicit confirmation of the validated preview', async () => {
  const imported = vi.fn().mockResolvedValue(undefined)
  vi.mocked(api.previewImport).mockResolvedValue({ token: 7, jumpers: 1, tunnels: 2, groups: 3, auto_start: 1, trusted_hosts: 1, bypass_jumpers: 0, auto_run: false })
  vi.mocked(api.confirmImport).mockResolvedValue([])
  render(<ConfigFiles onImported={imported} />)
  fireEvent.change(screen.getByLabelText('Configuration file'), { target: { value: 'C:\\fixture\\import.toml' } })
  fireEvent.click(screen.getByRole('button', { name: 'Import configuration' }))
  const dialog = await screen.findByRole('dialog', { name: 'Confirm import' })
  expect(api.confirmImport).not.toHaveBeenCalled()
  fireEvent.click(within(dialog).getByRole('button', { name: 'Replace configuration' }))
  await waitFor(() => expect(imported).toHaveBeenCalledOnce())
  expect(api.confirmImport).toHaveBeenCalledWith(7)
  expect(api.chooseConfigFile).not.toHaveBeenCalled()
})

test('cancelling a picker or export confirmation does not write files', async () => {
  vi.mocked(api.chooseConfigFile).mockResolvedValueOnce(null).mockResolvedValueOnce('C:\\fixture\\export.toml')
  render(<ConfigFiles onImported={vi.fn()} />)
  fireEvent.click(screen.getByRole('button', { name: 'Export configuration' }))
  await waitFor(() => expect(api.chooseConfigFile).toHaveBeenCalledOnce())
  await waitFor(() => expect((screen.getByRole('button', { name: 'Export configuration' }) as HTMLButtonElement).disabled).toBe(false))
  expect(screen.queryByRole('dialog')).toBeNull()
  fireEvent.click(screen.getByRole('button', { name: 'Export configuration' }))
  const dialog = await screen.findByRole('dialog', { name: 'Confirm export' })
  fireEvent.click(within(dialog).getAllByRole('button', { name: 'Cancel' }).at(-1)!)
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(api.exportConfig).not.toHaveBeenCalled()
})
