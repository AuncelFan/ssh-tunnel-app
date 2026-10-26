// @vitest-environment jsdom
import React from 'react'
import { afterEach, expect, test, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { CopyErrorButton } from './CopyErrorButton'
import { copyText } from '../journal'
vi.mock('../journal', () => ({ copyText: vi.fn() }))
afterEach(() => { cleanup(); vi.resetAllMocks() })
test('copy feedback applies to the copied error, not a later error', async () => {
  vi.mocked(copyText).mockResolvedValue(undefined)
  const view = render(<CopyErrorButton text="First error" />)
  fireEvent.click(screen.getByRole('button', { name: 'Copy error' }))
  await screen.findByRole('button', { name: 'Copied' })
  expect(copyText).toHaveBeenCalledWith('First error')
  view.rerender(<CopyErrorButton text="Later error" />)
  expect(screen.getByRole('button', { name: 'Copy error' })).toBeTruthy()
})
test('copy failure never reports copied', async () => {
  vi.mocked(copyText).mockRejectedValue(new Error('Unavailable'))
  render(<CopyErrorButton text="First error" />)
  fireEvent.click(screen.getByRole('button', { name: 'Copy error' }))
  await screen.findByRole('alert')
  expect(screen.queryByRole('button', { name: 'Copied' })).toBeNull()
})
