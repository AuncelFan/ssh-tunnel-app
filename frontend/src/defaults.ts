import type { Jumper, Tunnel } from './types'

export const newJumper = (): Jumper => ({ id: 0, name: '', host: '', port: 22, user: '', auth_type: 'ssh_key', key_path: '', agent_socket_path: '', password: '', bypass_host_verification: false, keep_alive_interval_ms: 30000, timeout_ms: 5000, host_key_algorithms: '', notes: '' })
export const newTunnel = (): Tunnel => ({ id: 0, name: '', group_id: 0, mode: 'local', jumper_ids: [], local_host: '127.0.0.1', local_port: 8080, remote_host: '127.0.0.1', remote_port: 80, auto_start: false, status: 'stopped', last_error: '', description: '', reconnect_mode: 'fixed', reconnect_interval_seconds: 5, reconnect_max_attempts: 0 })
