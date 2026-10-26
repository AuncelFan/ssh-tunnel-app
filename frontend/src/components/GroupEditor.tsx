import { useState } from 'react'
import type { Group, Tunnel } from '../types'
import { Modal } from './Modal'
import { useLocale } from '../locale'

export function GroupEditor({ groups, tunnels, save, remove, reorder, close }: {
  groups: Group[]; tunnels: Tunnel[]; save: (group: Group) => Promise<void>; remove: (id: number) => Promise<void>; reorder: (ids: number[]) => Promise<void>; close: () => void
}) {
  const { t } = useLocale()
  const [draft, setDraft] = useState<Group>({ id: 0, name: '', tunnel_ids: [] })
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  async function action(operation: () => Promise<void>, reset = true) {
    if (busy) return
    setBusy(true); setError('')
    try { await operation(); if (reset) setDraft({ id: 0, name: '', tunnel_ids: [] }) } catch (error) { setError(String(error)) } finally { setBusy(false) }
  }
  const selectedIndex = groups.findIndex(group => group.id === draft.id)
  function move(delta: number) {
    if (selectedIndex < 0 || selectedIndex + delta < 0 || selectedIndex + delta >= groups.length) return
    const ids = groups.map(group => group.id)
    ;[ids[selectedIndex], ids[selectedIndex + delta]] = [ids[selectedIndex + delta], ids[selectedIndex]]
    void action(() => reorder(ids), false)
  }
  return <Modal title={t("Manage groups")} closeLabel={t("Close group manager")} onClose={() => { if (!busy) close() }}>
    <p className="notice">{t("Custom groups are filters. A tunnel can belong to more than one custom group. Jumper groups are managed automatically.")}</p>
    <div className="group-pills"><button disabled={busy} onClick={() => setDraft({ id: 0, name: '', tunnel_ids: [] })}>{t("New group")}</button>{groups.map(group => <button key={group.id} disabled={busy} aria-pressed={draft.id === group.id} onClick={() => setDraft({ ...group, tunnel_ids: [...group.tunnel_ids] })}>{group.name}</button>)}</div>
    <form onSubmit={event => { event.preventDefault(); void action(() => save(draft)) }}><fieldset disabled={busy} className="editor-fields">
      {selectedIndex >= 0 && <div className="actions"><button type="button" disabled={selectedIndex === 0} onClick={() => move(-1)}>{t('Move up')}</button><button type="button" disabled={selectedIndex === groups.length - 1} onClick={() => move(1)}>{t('Move down')}</button></div>}
      <label>{t("Group name")}<input required value={draft.name} onChange={e => setDraft({ ...draft, name: e.target.value })} /></label>
      <div className="membership-list">{tunnels.map(tunnel => <label key={tunnel.id}><input type="checkbox" checked={draft.tunnel_ids.includes(tunnel.id)} onChange={e => setDraft({ ...draft, tunnel_ids: e.target.checked ? [...draft.tunnel_ids, tunnel.id] : draft.tunnel_ids.filter(id => id !== tunnel.id) })} /><span>{tunnel.name}</span></label>)}</div>
      {error && <p role="alert" className="error-detail">{error}</p>}
      <div className="dialog-actions">{draft.id !== 0 && <button className="danger" type="button" onClick={() => { void action(() => remove(draft.id)) }}>{t("Delete group")}</button>}<button className="primary" type="submit">{t("Save group")}</button></div>
    </fieldset></form>
  </Modal>
}
