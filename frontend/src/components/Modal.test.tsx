// @vitest-environment jsdom
import React from 'react'
import { afterEach, beforeAll, expect, test, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { Modal } from './Modal'

beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', '') }
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open') }
})
afterEach(() => { cleanup(); vi.restoreAllMocks() })

function backdropClick(dialog: HTMLElement) {
  vi.spyOn(dialog, 'getBoundingClientRect').mockReturnValue({ left: 100, right: 500, top: 100, bottom: 500 } as DOMRect)
  // MouseEvent provides coordinates in jsdom versions without native PointerEvent.
  fireEvent(dialog, new MouseEvent('pointerdown', { bubbles: true, clientX: 20, clientY: 20 }))
  fireEvent.click(dialog, { clientX: 20, clientY: 20 })
}

test('detail dialog is portalled and backdrop dismisses it', () => {
  const close = vi.fn()
  const { container } = render(<Modal title="Details" closeLabel="Close" closeOnBackdrop onClose={close}>Content</Modal>)
  const dialog = screen.getByRole('dialog', { name: 'Details' })
  expect(container.contains(dialog)).toBe(false)
  backdropClick(dialog)
  expect(close).toHaveBeenCalledOnce()
})

test('edit dialog does not dismiss on backdrop but has an explicit close button', () => {
  const close = vi.fn()
  render(<Modal title="Edit" closeLabel="Close" onClose={close}>Form</Modal>)
  backdropClick(screen.getByRole('dialog', { name: 'Edit' }))
  expect(close).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole('button', { name: 'Close' }))
  expect(close).toHaveBeenCalledOnce()
})

test('clicking the dialog interior does not dismiss details', () => {
  const close = vi.fn()
  render(<Modal title="Details" closeLabel="Close" closeOnBackdrop onClose={close}>Content</Modal>)
  const dialog = screen.getByRole('dialog')
  vi.spyOn(dialog, 'getBoundingClientRect').mockReturnValue({ left: 0, top: 0, right: 500, bottom: 500 } as DOMRect)
  fireEvent(dialog, new MouseEvent('pointerdown', { bubbles: true, clientX: 20, clientY: 20 }))
  fireEvent.click(dialog, { clientX: 20, clientY: 20 })
  expect(close).not.toHaveBeenCalled()
})
