import { test } from 'node:test'
import assert from 'node:assert/strict'
import { resolveUserFolders } from './native-ci-profile.mjs'

test('fresh account AppData stays under its own known profile', () => {
  assert.deepEqual(resolveUserFolders({ profile: 'C:\\Users\\fixture', local: '', roaming: '' }), {
    folders: { profile: 'C:\\Users\\fixture', local: 'C:\\Users\\fixture\\AppData\\Local', roaming: 'C:\\Users\\fixture\\AppData\\Roaming' },
    initializedAppData: ['local', 'roaming'],
  })
})
test('existing redirected known folders remain authoritative', () => {
  const identity = { profile: 'C:\\Users\\fixture', local: 'D:\\Local', roaming: 'D:\\Roaming' }
  assert.deepEqual(resolveUserFolders(identity), { folders: identity, initializedAppData: [] })
})
test('missing profile and nonempty relative known folders fail closed', () => {
  for (const profile of ['', 'relative']) assert.throws(() => resolveUserFolders({ profile }))
  assert.throws(() => resolveUserFolders({ profile: 'C:\\Users\\fixture', local: 'relative' }))
})
