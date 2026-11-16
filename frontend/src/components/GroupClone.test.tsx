// @vitest-environment jsdom
import React from 'react'
import { afterEach, beforeAll, expect, test, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { GroupClone } from './GroupClone'
import { newJumper, newTunnel } from '../defaults'
import type { Filter } from '../types'

beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', '') }
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open') }
})
afterEach(cleanup)
const jumpers = [1, 2, 3].map(id => ({ ...newJumper(), id, name: `Machine ${id}`, host: `machine-${id}.invalid`, user: 'fixture' }))
const source: Filter = { key: 'custom-9', kind: 'custom', name: 'Services', tunnels: [
  { ...newTunnel(), id: 10, name: 'Database', jumper_ids: [1, 3] },
  { ...newTunnel(), id: 11, name: 'Cache', jumper_ids: [1] },
] }

test('clones every group member and excludes the existing first hop and later hops', async () => {
  const save = vi.fn(async () => {})
  const close = vi.fn()
  render(<GroupClone source={source} jumpers={jumpers} save={save} close={close} />)
  expect(screen.getAllByRole('option').map(option => option.textContent)).toEqual(['Select a jumper', 'Machine 2 (fixture@machine-2.invalid:22)'])
  expect(screen.getByText('Database')).toBeTruthy()
  expect(screen.getByText('Cache')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Clone group' }))
  await waitFor(() => expect(save).toHaveBeenCalledWith({ name: 'Services → Machine 2', tunnel_ids: [10, 11], target_jumper_id: 2 }))
  expect(close).toHaveBeenCalledOnce()
  expect(source.tunnels[0].jumper_ids).toEqual([1, 3])
})

test('a failed atomic save keeps the dialog and edited name for retry', async () => {
  const save = vi.fn().mockRejectedValue(new Error('A source tunnel no longer exists'))
  const close = vi.fn()
  render(<GroupClone source={source} jumpers={jumpers} save={save} close={close} />)
  fireEvent.change(screen.getByLabelText('Group name'), { target: { value: 'Deployment B' } })
  fireEvent.click(screen.getByRole('button', { name: 'Clone group' }))
  await screen.findByRole('alert')
  expect(close).not.toHaveBeenCalled()
  expect((screen.getByLabelText('Group name') as HTMLInputElement).value).toBe('Deployment B')
  expect((screen.getByRole('button', { name: 'Clone group' }) as HTMLButtonElement).disabled).toBe(false)
})

test('missing alternative machines cannot submit a clone', () => {
  const save = vi.fn()
  render(<GroupClone source={source} jumpers={jumpers.filter(jumper => jumper.id !== 2)} save={save} close={() => {}} />)
  expect(screen.getByText('Add another jumper that does not already occur later in these chains.')).toBeTruthy()
  expect((screen.getByRole('button', { name: 'Clone group' }) as HTMLButtonElement).disabled).toBe(true)
  fireEvent.submit(screen.getByLabelText('Group name').closest('form')!)
  expect(save).not.toHaveBeenCalled()
})
