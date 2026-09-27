import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-shell';

const MYACCOUNT_URL = 'https://myaccount.tayyem.dev';

export const api = {
  login: (usernameOrEmail, password, remember) => invoke('auth_login', { usernameOrEmail, password, remember }),
  logout: () => invoke('auth_logout'),
  restoreSession: () => invoke('auth_restore'),

  openMyAccount: (path = '/profile') => open(`${MYACCOUNT_URL}${path}`),
  openForgotPassword: () => open(`${MYACCOUNT_URL}/forgot-password`),

  connect: () => invoke('vpn_connect'),
  disconnect: () => invoke('vpn_disconnect'),
  getStatus: () => invoke('vpn_get_status'),
  onStatus: (cb) => listen('vpn:status', (event) => cb(event.payload)),
  onLog: (cb) => listen('vpn:log', (event) => cb(event.payload)),

  getSplitTunnelConfig: () => invoke('split_tunnel_get_config'),
  setSplitTunnelConfig: (config) => invoke('split_tunnel_set_config', { config }),
  listCandidateApps: () => invoke('split_tunnel_list_candidate_apps'),

  listDevices: () => invoke('devices_list'),
  requestDeviceVerification: (body) => invoke('devices_request_verification', { body }),
  confirmDeviceVerification: (code) => invoke('devices_confirm_verification', { code }),
  getSubscription: () => invoke('subscription_me'),

  listFiles: (path) => invoke('files_list', { path }),
  getFileUsage: () => invoke('files_usage'),
  createFolder: (path) => invoke('files_create_folder', { path }),
  deleteFolder: (path) => invoke('files_delete_folder', { path }),
  deleteFile: (id) => invoke('files_delete', { id }),
  uploadFile: (path, filename, contentType, dataBase64) =>
    invoke('files_upload', { path, filename, contentType, dataBase64 }),
  /** Resolves to a base64 string — the caller decodes it into a Blob to save/preview. */
  downloadFile: (id) => invoke('files_download', { id }),
};
