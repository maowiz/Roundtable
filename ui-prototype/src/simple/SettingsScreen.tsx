// Settings on one page: keys, defaults, about. Agent prompts live on each agent's card.

import { useEffect, useState } from 'react';
import { Moon, Sun } from 'lucide-react';
import { api, type AppInfo } from '../api/client';
import { Connections } from '../desktop';
import type { AppSettings } from './state';

export default function SettingsScreen({ settings, update, notify, onKeysChanged }: {
  settings: AppSettings;
  update: (p: Partial<AppSettings>) => void;
  notify: (m: string) => void;
  onKeysChanged: () => void;
}) {
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => { api.appInfo().then(setInfo).catch(() => {}); }, []);

  return <div className="rt-page">
    <h1 className="rt-title">Settings</h1>

    <section className="rt-card" onBlur={onKeysChanged}>
      <h2>API keys</h2>
      <p className="rt-hint">Keys live in Windows Credential Manager. Models from every provider listed here appear in each agent’s model menu.</p>
      <Connections notify={m => { notify(m); onKeysChanged(); }}/>
    </section>

    <section className="rt-card">
      <h2>Defaults</h2>
      <div className="rt-row">
        <label className="rt-inline">Spending limit per run $<input className="rt-input rt-small" value={settings.ceilingUsd} onChange={e => update({ ceilingUsd: e.target.value })} aria-label="Default spending limit"/></label>
        <span className="rt-inline">Theme
          <button className={'rt-chip' + (settings.theme === 'dark' ? ' on' : '')} onClick={() => update({ theme: 'dark' })}><Moon size={13}/> Dark</button>
          <button className={'rt-chip' + (settings.theme === 'light' ? ' on' : '')} onClick={() => update({ theme: 'light' })}><Sun size={13}/> Light</button>
        </span>
      </div>
    </section>

    {info && <section className="rt-card">
      <h2>About</h2>
      <p className="rt-hint">Roundtable {info.appVersion} · prices {info.priceCatalog} · data in {info.dataDir}</p>
    </section>}
  </div>;
}
