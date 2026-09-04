import { openUrl } from '@tauri-apps/plugin-opener';

export const openEndpoint = (url: string) => {
  openUrl(url).catch(() => window.open(url, '_blank'));
};
