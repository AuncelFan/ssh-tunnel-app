// @vitest-environment jsdom
import React from 'react'
import { afterEach, expect, test, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { Toggle } from './Toggle'

afterEach(cleanup)
test('switch has an accessible label and reports the next checked state', () => {
  const onChange = vi.fn()
  render(<Toggle label="Show jumper groups" checked={false} onChange={onChange} />)
  const input = screen.getByRole('switch', { name: 'Show jumper groups' })
  fireEvent.click(input)
  expect(onChange).toHaveBeenCalledWith(true)
})
