// @vitest-environment jsdom
import React from 'react'
import { afterEach, expect, test, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { api } from '../api'
import { LogsPage } from './LogsPage'

vi.mock('../api', () => ({ api: { activityLogs: vi.fn() } }))
afterEach(() => { cleanup(); vi.resetAllMocks() })
test('the log page exposes info, warning and error filters with timestamps', async () => {
  const base = { tunnel_id: 1, generation: 1, timestamp_ms: 1700000000000, challenge: null }
  vi.mocked(api.activityLogs).mockResolvedValue([
    { ...base, kind: 'error', message: 'Synthetic network error' },
    { ...base, kind: 'status', message: 'reconnecting' },
    { ...base, kind: 'action', message: 'save_jumper', tunnel_id: 0 },
  ])
  render(<LogsPage />)
  const log = screen.getByRole('log')
  await within(log).findByText('Save jumper: Succeeded')
  expect(log.querySelectorAll('time')).toHaveLength(3)
  fireEvent.click(screen.getByRole('button', { name: 'Errors' }))
  expect(within(log).getByText('Synthetic network error')).toBeTruthy()
  expect(within(log).queryByText('Save jumper: Succeeded')).toBeNull()
  fireEvent.click(screen.getByRole('button', { name: 'Warnings' }))
  expect(within(log).queryByText('Synthetic network error')).toBeNull()
  expect(log.querySelectorAll('.journal-entry')).toHaveLength(1)
  fireEvent.click(screen.getByRole('button', { name: 'Info' }))
  expect(within(log).getByText('Save jumper: Succeeded')).toBeTruthy()
})
test('native log failures remain visible and do not render a false success', async () => {
  vi.mocked(api.activityLogs).mockRejectedValue(new Error('Native journal unavailable'))
  render(<LogsPage />)
  expect((await screen.findByRole('alert')).textContent).toContain('Native journal unavailable')
})
