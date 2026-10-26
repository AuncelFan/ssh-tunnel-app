// @vitest-environment jsdom
import React from 'react'
import { afterEach, beforeAll, expect, test, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { HostTrustDialog } from './HostTrustDialog'
import type { RuntimeEvent } from '../types'

beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', '') }
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open') }
})
afterEach(cleanup)
const event: RuntimeEvent = { tunnel_id: 1, generation: 5, kind: 'host_key', message: '', challenge: { jumper_id: 2, host: 'example.org', port: 2222, previous_fingerprint: null, fingerprint: 'SHA256:new', public_key: 'fixture-public-key' } }

test('first trust shows endpoint and fingerprint and requires explicit approval', async () => {
  const respond = vi.fn().mockResolvedValue(undefined)
  render(<HostTrustDialog event={event} respond={respond} onError={vi.fn()} />)
  expect(screen.getByText('example.org:2222')).toBeTruthy()
  expect(screen.getByText('SHA256:new')).toBeTruthy()
  expect(respond).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole('button', { name: 'Trust key and connect' }))
  await waitFor(() => expect(respond).toHaveBeenCalledWith(event, true))
})

test('changed key displays both fingerprints and uses replacement wording', async () => {
  const changed = { ...event, challenge: { ...event.challenge!, previous_fingerprint: 'SHA256:old' } }
  const respond = vi.fn().mockResolvedValue(undefined)
  render(<HostTrustDialog event={changed} respond={respond} onError={vi.fn()} />)
  expect(screen.getByRole('dialog', { name: 'Server identity changed' })).toBeTruthy()
  expect(screen.getByText('SHA256:old')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Replace saved key and reconnect' }))
  await waitFor(() => expect(respond).toHaveBeenCalledWith(changed, true))
})
