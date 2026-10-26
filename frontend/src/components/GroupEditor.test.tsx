// @vitest-environment jsdom
import React from 'react'
import { afterEach, beforeAll, expect, test, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { GroupEditor } from './GroupEditor'
import { newTunnel } from '../defaults'

beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', '') }
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open') }
})
afterEach(cleanup)
test('editing one custom filter changes only its own membership', async () => {
  const save = vi.fn().mockResolvedValue(undefined)
  const groups = [{ id: 1, name: 'A', tunnel_ids: [1] }, { id: 2, name: 'B', tunnel_ids: [1] }]
  render(<GroupEditor groups={groups} tunnels={[{ ...newTunnel(), id: 1, name: 'Service' }]} save={save} remove={vi.fn()} reorder={vi.fn()} close={vi.fn()} />)
  fireEvent.click(screen.getByRole('button', { name: 'A' }))
  fireEvent.click(screen.getByRole('checkbox', { name: 'Service' }))
  fireEvent.click(screen.getByRole('button', { name: 'Save group' }))
  await waitFor(() => expect(save).toHaveBeenCalledWith({ id: 1, name: 'A', tunnel_ids: [] }))
  expect(groups[1].tunnel_ids).toEqual([1])
})

test('moving a group preserves unsaved membership and reports failures', async () => {
  const reorder = vi.fn().mockRejectedValue(new Error('Disk unavailable'))
  const groups = [{ id: 1, name: 'A', tunnel_ids: [1] }, { id: 2, name: 'B', tunnel_ids: [1] }]
  const save = vi.fn().mockResolvedValue(undefined)
  render(<GroupEditor groups={groups} tunnels={[{ ...newTunnel(), id: 1, name: 'Service' }]} save={save} remove={vi.fn()} reorder={reorder} close={vi.fn()} />)
  fireEvent.click(screen.getByRole('button', { name: 'A' }))
  expect((screen.getByRole('button', { name: 'Move up' }) as HTMLButtonElement).disabled).toBe(true)
  fireEvent.change(screen.getByLabelText('Group name'), { target: { value: 'Edited' } })
  fireEvent.click(screen.getByRole('checkbox', { name: 'Service' }))
  fireEvent.click(screen.getByRole('button', { name: 'Move down' }))
  await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('Disk unavailable'))
  expect(reorder).toHaveBeenCalledWith([2, 1])
  fireEvent.click(screen.getByRole('button', { name: 'Save group' }))
  await waitFor(() => expect(save).toHaveBeenCalledWith({ id: 1, name: 'Edited', tunnel_ids: [] }))
})
