// @vitest-environment jsdom
import React from 'react'
import { afterEach, beforeAll, expect, test, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { JumperEditor, TunnelEditor } from './ConnectionEditor'
import { newJumper, newTunnel } from '../defaults'

beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', '') }
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open') }
})
afterEach(cleanup)

test('new tunnel uses loopback endpoints and saves the ordered jumper chain and retry policy', async () => {
  const save = vi.fn().mockResolvedValue(undefined)
  const jumpers = [{ ...newJumper(), id: 1, name: 'First' }, { ...newJumper(), id: 2, name: 'Second' }]
  render(<TunnelEditor createJumper={vi.fn()} initial={newTunnel()} jumpers={jumpers} save={save} close={vi.fn()} />)
  expect((screen.getByLabelText('Local host') as HTMLInputElement).value).toBe('127.0.0.1')
  expect((screen.getByLabelText('Remote host') as HTMLInputElement).value).toBe('127.0.0.1')
  fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Service' } })
  fireEvent.change(screen.getByLabelText('Add jumper'), { target: { value: '1' } })
  fireEvent.change(screen.getByLabelText('Add jumper'), { target: { value: '2' } })
  fireEvent.click(screen.getByRole('button', { name: 'Move up 2' }))
  fireEvent.change(screen.getByLabelText('Reconnect policy'), { target: { value: 'exponential' } })
  fireEvent.change(screen.getByLabelText('Retry interval (seconds)'), { target: { value: '10' } })
  fireEvent.click(screen.getByRole('button', { name: 'Save tunnel' }))
  await waitFor(() => expect(save).toHaveBeenCalledOnce())
  expect(save.mock.calls[0][0]).toMatchObject({ name: 'Service', jumper_ids: [2, 1], reconnect_mode: 'exponential', reconnect_interval_seconds: 10, reconnect_max_attempts: 0 })
})

test('password and agent fields switch without accidentally enabling host bypass', async () => {
  const save = vi.fn().mockResolvedValue(undefined)
  const value = { ...newJumper(), name: 'Server', host: 'localhost', user: 'test' }
  render(<JumperEditor initial={value} save={save} close={vi.fn()} />)
  fireEvent.change(screen.getByLabelText('Authentication'), { target: { value: 'ssh_agent' } })
  expect(screen.queryByLabelText('Private key path')).toBeNull()
  expect(screen.getByLabelText('Agent socket / named pipe')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Save jumper' }))
  await waitFor(() => expect(save).toHaveBeenCalledOnce())
  expect(save.mock.calls[0][0].bypass_host_verification).toBe(false)
})

test('inline jumper failure preserves both drafts and successful retry appends the hop', async () => {
  const save = vi.fn().mockResolvedValue(undefined)
  const create = vi.fn().mockRejectedValueOnce(new Error('Write failed')).mockResolvedValue(7)
  render(<TunnelEditor initial={{ ...newTunnel(), name: 'Draft tunnel', jumper_ids: [1] }} jumpers={[{ ...newJumper(), id: 1, name: 'Existing' }]} save={save} createJumper={create} close={vi.fn()} />)
  fireEvent.click(screen.getByRole('button', { name: 'New jumper' }))
  const inline = within(screen.getByRole('dialog', { name: 'New jumper' }))
  fireEvent.change(inline.getByLabelText('Name'), { target: { value: 'Inline host' } })
  fireEvent.change(inline.getByLabelText('Host'), { target: { value: '127.0.0.1' } })
  fireEvent.change(inline.getByLabelText('User'), { target: { value: 'fixture' } })
  fireEvent.change(inline.getByLabelText('Authentication'), { target: { value: 'ssh_agent' } })
  fireEvent.click(inline.getByRole('button', { name: 'Save jumper' }))
  await waitFor(() => expect(inline.getByRole('alert').textContent).toContain('Write failed'))
  expect(save).not.toHaveBeenCalled()
  fireEvent.click(inline.getByRole('button', { name: 'Save jumper' }))
  await waitFor(() => expect(screen.queryByRole('dialog', { name: 'New jumper' })).toBeNull())
  fireEvent.click(screen.getByRole('button', { name: 'Save tunnel' }))
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ name: 'Draft tunnel', jumper_ids: [1, 7] })))
})

test('cancelling inline jumper creation does not submit or discard the tunnel', () => {
  const create = vi.fn()
  render(<TunnelEditor initial={{ ...newTunnel(), name: 'Draft' }} jumpers={[]} save={vi.fn()} createJumper={create} close={vi.fn()} />)
  fireEvent.click(screen.getByRole('button', { name: 'New jumper' }))
  fireEvent.click(within(screen.getByRole('dialog', { name: 'New jumper' })).getByRole('button', { name: 'Cancel' }))
  expect((screen.getByLabelText('Name') as HTMLInputElement).value).toBe('Draft')
  expect(create).not.toHaveBeenCalled()
})
