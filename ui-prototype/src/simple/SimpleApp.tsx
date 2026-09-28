// The installed desktop app: three places only. Ask (with runs), History, Settings.

import { useCallback, useRef, useState } from 'react';
import { History, MessageSquare, Settings } from 'lucide-react';
import AskScreen from './AskScreen';
import HistoryScreen from './HistoryScreen';
import RunView from './RunView';
import SettingsScreen from './SettingsScreen';
import { useAppSettings, useTeam } from './state';
import './simple.css';

type View = { kind: 'ask' } | { kind: 'run'; id: string } | { kind: 'history' } | { kind: 'settings' };
interface Toast { id: number; text: string; action?: { label: string; run: () => void } }

function Logo() {
  return <svg className="rt-logo" viewBox="0 0 40 40" fill="none" aria-hidden="true"><circle cx="20" cy="20" r="8.5" stroke="currentColor" strokeWidth="2.4"/><circle cx="20" cy="5" r="2.6" fill="currentColor"/><circle cx="20" cy="35" r="2.6" fill="currentColor"/><circle cx="5" cy="20" r="2.6" fill="currentColor"/><circle cx="35" cy="20" r="2.6" fill="currentColor"/></svg>;
}

export default function SimpleApp() {
  const [view, setView] = useState<View>({ kind: 'ask' });
  const [toastState, setToast] = useState<Toast | null>(null);
  const hide = useRef<number | undefined>(undefined);

  const toast = useCallback((text: string, action?: Toast['action']) => {
    window.clearTimeout(hide.current);
    setToast({ id: Date.now(), text, action });
    hide.current = window.setTimeout(() => setToast(null), action ? 7000 : 4000);
  }, []);
  const notify = useCallback((m: string) => toast(m), [toast]);

  const teamApi = useTeam(notify);
  const { settings, update } = useAppSettings();

  const tab = view.kind === 'run' ? 'ask' : view.kind;
  const nav = [
    { kind: 'ask' as const, label: 'Ask', icon: <MessageSquare size={16}/> },
    { kind: 'history' as const, label: 'History', icon: <History size={16}/> },
    { kind: 'settings' as const, label: 'Settings', icon: <Settings size={16}/> },
  ];

  return <div className="rt-app">
    <header className="rt-top">
      <div className="rt-brand"><Logo/> Roundtable</div>
      <nav className="rt-nav" aria-label="Main">{nav.map(n => <button key={n.kind} className={tab === n.kind ? 'on' : ''} aria-current={tab === n.kind ? 'page' : undefined} onClick={() => setView({ kind: n.kind })}>{n.icon}{n.label}</button>)}</nav>
    </header>

    <main className="rt-main">
      {view.kind === 'ask' && <AskScreen teamApi={teamApi} ceilingUsd={settings.ceilingUsd} setCeilingUsd={v => update({ ceilingUsd: v })} notify={notify} toast={toast} onStarted={id => setView({ kind: 'run', id })}/>}
      {view.kind === 'run' && <RunView runId={view.id} notify={notify} onNew={() => setView({ kind: 'ask' })}/>}
      {view.kind === 'history' && <HistoryScreen onOpen={id => setView({ kind: 'run', id })} notify={notify}/>}
      {view.kind === 'settings' && <SettingsScreen settings={settings} update={update} notify={notify} onKeysChanged={() => void teamApi.reload()}/>}
    </main>

    {toastState && <div className="rt-toast" role="status" key={toastState.id}>
      <span>{toastState.text}</span>
      {toastState.action && <button onClick={() => { toastState.action!.run(); setToast(null); }}>{toastState.action.label}</button>}
      <button aria-label="Dismiss" onClick={() => setToast(null)}>×</button>
    </div>}
  </div>;
}
