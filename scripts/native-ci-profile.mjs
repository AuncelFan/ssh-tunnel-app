import { win32 as path } from 'node:path'

export function resolveUserFolders(identity) {
  if (!identity.profile || !path.isAbsolute(identity.profile)) throw new Error('Native CI worker has no usable Windows user profile')
  const folders = { profile: identity.profile, local: identity.local, roaming: identity.roaming }
  const initializedAppData = []
  for (const [key, folder] of [['local', 'Local'], ['roaming', 'Roaming']]) {
    if (!folders[key]) {
      folders[key] = path.join(folders.profile, 'AppData', folder)
      initializedAppData.push(key)
    }
    if (!path.isAbsolute(folders[key])) throw new Error('Native CI worker received a relative Windows known folder')
  }
  return { folders, initializedAppData }
}
