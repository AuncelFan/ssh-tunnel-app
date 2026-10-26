import type { Config, Filter } from './types'

export function filters(config: Config, showJumpers: boolean, allLabel: string): Filter[] {
  const result: Filter[] = [{ key: 'all', name: allLabel, kind: 'all', tunnels: config.tunnels }]
  if (showJumpers) for (const host of config.jumpers) {
    result.push({ key: `jumper-${host.id}`, name: host.name, kind: 'jumper', tunnels: config.tunnels.filter(t => t.jumper_ids[0] === host.id) })
  }
  for (const group of config.groups) {
    result.push({ key: `custom-${group.id}`, name: group.name, kind: 'custom', tunnels: config.tunnels.filter(t => group.tunnel_ids.includes(t.id)) })
  }
  return result
}

export function selectedFilter(items: Filter[], key: string): Filter {
  return items.find(item => item.key === key) ?? items[0]
}
