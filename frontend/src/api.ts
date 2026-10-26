import { invoke } from '@tauri-apps/api/core'
import type { Config, Group, Jumper, Tunnel, RuntimeEvent, TunnelRuntime, HostKeyChallenge } from './types'

export interface ImportPreview { token: number; jumpers: number; tunnels: number; groups: number; trusted_hosts: number; bypass_jumpers: number; auto_start: number; auto_run: boolean }
export interface ConfigLocation { implicit: string; effective: string; pointer: string }
export interface DirectoryConflict { config_exists: boolean; locale_exists: boolean }
export interface ProbeRequest { jumpers: Jumper[]; tunnel: Tunnel | null; diagnostics: boolean }
export interface ProbeReport { success: boolean; elapsed_ms: number; latency_ms: number | null; checks: { phase: string; ok: boolean; code: string; detail: string }[]; challenge: HostKeyChallenge | null }
export interface SSHConfigCandidate { alias: string; jumper: Jumper; proxy_jump: string; warnings: string[] }
export interface SSHConfigPreview { candidates: SSHConfigCandidate[]; warnings: string[] }
export interface BatchImport { imported: number[]; skipped: number }
export interface CommandPreview { jumpers: Jumper[]; tunnels: Tunnel[]; warnings: string[] }
export interface TunnelBatchImport extends BatchImport { created_jumpers: number[] }

// No production mock fallback: unavailable native bindings must be visible.
export const api = {
  beginProbe: (request: ProbeRequest) => invoke<number>('begin_probe', { request }),
  runProbe: (token: number) => invoke<ProbeReport>('run_probe', { token }),
  approveProbeKey: (token: number) => invoke<void>('approve_probe_key', { token }),
  cancelProbe: (token: number) => invoke<void>('cancel_probe', { token }),
  configLocation: () => invoke<ConfigLocation>('get_config_location'),
  directoryConflicts: (target: string | null) => invoke<DirectoryConflict>('config_directory_conflicts', { target }),
  relocateConfig: (target: string | null, overwrite: boolean) => invoke<string[]>('relocate_config', { target, overwrite }),
  chooseConfigDirectory: () => invoke<string | null>('choose_config_directory'),
  revealConfigDirectory: () => invoke<void>('reveal_config_directory'),
  parseSSHCommand: (command: string) => invoke<CommandPreview>('parse_ssh_command', { command }),
  importCommandTunnels: (request: { jumpers: Jumper[]; tunnels: Tunnel[] }) => invoke<TunnelBatchImport>('import_command_tunnels', { request }),
  defaultSSHConfigPath: () => invoke<string>('default_ssh_config_path'),
  loadSSHConfig: (path: string) => invoke<SSHConfigPreview>('load_ssh_config', { path }),
  getAutoRun: () => invoke<boolean>('get_auto_run'),
  setAutoRun: (enabled: boolean) => invoke<void>('set_auto_run', { enabled }),
  configPath: () => invoke<string>('get_config_path'),
  chooseConfigFile: (save: boolean) => invoke<string | null>('choose_config_file', { save }),
  previewImport: (path: string) => invoke<ImportPreview>('preview_import', { path }),
  confirmImport: (token: number) => invoke<string[]>('confirm_import', { token }),
  cancelImport: () => invoke<void>('cancel_import'),
  exportConfig: (path: string) => invoke<void>('export_config', { path }),
  getUILocale: () => invoke<string>('get_ui_locale'),
  setUILocale: (locale: string) => invoke<void>('set_ui_locale', { locale }),
  takeStartupErrors: () => invoke<string[]>('take_startup_errors'),
  quit: () => invoke<void>('quit_app'),
  setTrafficMonitor: (enabled: boolean) => invoke<void>('set_traffic_monitor', { enabled }),
  getState: () => invoke<Config>('get_state'),
  saveJumper: (jumper: Jumper) => invoke<number>('save_jumper', { jumper }),
  importJumpers: (jumpers: Jumper[]) => invoke<BatchImport>('import_jumpers', { jumpers }),
  deleteJumper: (id: number) => invoke<void>('delete_jumper', { id }),
  saveTunnel: (tunnel: Tunnel) => invoke<number>('save_tunnel', { tunnel }),
  deleteTunnel: (id: number) => invoke<void>('delete_tunnel', { id }),
  saveGroup: (group: Group) => invoke<number>('save_group', { group }),
  deleteGroup: (id: number) => invoke<void>('delete_group', { id }),
  reorderGroups: (ids: number[]) => invoke<void>('reorder_groups', { ids }),
  startTunnel: (id: number) => invoke<void>('start_tunnel', { id }),
  stopTunnel: (id: number) => invoke<void>('stop_tunnel', { id }),
  runtime: () => invoke<TunnelRuntime[]>('get_runtime'),
  logs: (id: number) => invoke<RuntimeEvent[]>('get_tunnel_logs', { id }),
  activityLogs: () => invoke<RuntimeEvent[]>('get_activity_logs'),
  pendingTrust: () => invoke<RuntimeEvent[]>('get_pending_trust'),
  resolveHostKey: (id: number, generation: number, accept: boolean) => invoke<void>('resolve_host_key', { id, generation, accept }),
}
