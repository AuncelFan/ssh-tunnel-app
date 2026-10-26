// @vitest-environment jsdom
import React from 'react'
import { afterEach, beforeAll, beforeEach, expect, test, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { api } from '../api'
import { ConfigDirectory } from './ConfigDirectory'

vi.mock('../api', () => ({ api: { directoryConflicts: vi.fn(), relocateConfig: vi.fn(), chooseConfigDirectory: vi.fn(), revealConfigDirectory: vi.fn(), getUILocale: vi.fn() } }))
beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', '') }
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open') }
})
beforeEach(() => {
  vi.mocked(api.directoryConflicts).mockResolvedValue({ config_exists: true, locale_exists: true })
  vi.mocked(api.relocateConfig).mockResolvedValue([])
  vi.mocked(api.getUILocale).mockResolvedValue('en')
})
afterEach(() => { cleanup(); vi.resetAllMocks() })

test('destination settings require confirmation; cancellation does not relocate', async () => {
  const changed = vi.fn().mockResolvedValue(undefined)
  render(<ConfigDirectory onChanged={changed} />)
  fireEvent.change(screen.getByLabelText('Target directory'), { target: { value: 'C:\\fixture\\destination' } })
  fireEvent.click(screen.getByRole('button', { name: 'Change directory' }))
  let dialog = await screen.findByRole('dialog')
  expect(api.relocateConfig).not.toHaveBeenCalled()
  fireEvent.click(within(dialog).getAllByRole('button', { name: 'Cancel' }).at(-1)!)
  expect(screen.queryByRole('dialog')).toBeNull()
  fireEvent.click(screen.getByRole('button', { name: 'Change directory' }))
  dialog = await screen.findByRole('dialog')
  fireEvent.click(within(dialog).getByRole('button', { name: 'Use destination settings' }))
  await waitFor(() => expect(changed).toHaveBeenCalledOnce())
  expect(api.relocateConfig).toHaveBeenCalledWith('C:\\fixture\\destination', false)
})

test('reset uses the native anchor and failed writes leave confirmation open', async () => {
  vi.mocked(api.relocateConfig).mockRejectedValue(new Error('Read-only destination'))
  const changed = vi.fn()
  render(<ConfigDirectory onChanged={changed} />)
  fireEvent.click(screen.getByRole('button', { name: 'Restore default directory' }))
  const dialog = await screen.findByRole('dialog')
  expect(api.directoryConflicts).toHaveBeenCalledWith(null)
  fireEvent.click(within(dialog).getByRole('button', { name: 'Copy current settings' }))
  expect((await within(dialog).findByRole('alert')).textContent).toContain('Read-only destination')
  expect(api.relocateConfig).toHaveBeenCalledWith(null, true)
  expect(changed).not.toHaveBeenCalled()
})

test('cancelling the native folder picker makes no change', async () => {
  vi.mocked(api.chooseConfigDirectory).mockResolvedValue(null)
  render(<ConfigDirectory onChanged={vi.fn()} />)
  fireEvent.click(screen.getByRole('button', { name: 'Change directory' }))
  await waitFor(() => expect(api.chooseConfigDirectory).toHaveBeenCalledOnce())
  expect(api.directoryConflicts).not.toHaveBeenCalled()
  expect(api.relocateConfig).not.toHaveBeenCalled()
})
