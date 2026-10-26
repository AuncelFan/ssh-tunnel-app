import { useEffect, useId, useRef, type ReactNode } from 'react'
import { createPortal } from 'react-dom'

interface Props {
  title: string
  children: ReactNode
  onClose: () => void
  closeOnBackdrop?: boolean
  closeLabel: string
}

// A native top-layer dialog avoids clipping by cards, grids and scrolling ancestors.
export function Modal({ title, children, onClose, closeOnBackdrop = false, closeLabel }: Props) {
  const ref = useRef<HTMLDialogElement>(null)
  const pointerStartedOutside = useRef(false)
  const titleId = useId()
  useEffect(() => {
    const dialog = ref.current!
    dialog.showModal()
    return () => { dialog.close() }
  }, [])
  const outside = (event: { clientX: number; clientY: number }, dialog: HTMLDialogElement) => {
    const rect = dialog.getBoundingClientRect()
    return event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom
  }
  return createPortal(<dialog ref={ref} className="modal" aria-labelledby={titleId}
    onCancel={event => { event.preventDefault(); onClose() }}
    onPointerDown={event => { pointerStartedOutside.current = outside(event, event.currentTarget) }}
    onClick={event => {
      if (closeOnBackdrop && pointerStartedOutside.current && outside(event, event.currentTarget)) onClose()
      pointerStartedOutside.current = false
    }}>
    <div className="modal-heading"><h2 id={titleId}>{title}</h2><button type="button" onClick={onClose} aria-label={closeLabel}>{'\u00d7'}</button></div>
    <div className="modal-body">{children}</div>
  </dialog>, document.body)
}
