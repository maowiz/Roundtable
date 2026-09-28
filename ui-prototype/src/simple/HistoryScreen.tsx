// Past runs: search, open, delete.

import { useEffect, useState } from 'react';
import { Search, Trash2 } from 'lucide-react';
import { api, type RunSummary } from '../api/client';

const LABEL: Record<string, string> = { completed: 'Completed', cancelled: 'Stopped', failed: 'Failed', blocked: 'Paused', interrupted: 'Paused' };

export default function HistoryScreen({ onOpen, notify }: { onOpen: (id: string) => void; notify: (m: string) => void }) {
  const [runs, setRuns] = useState<RunSummary[] | null>(null);
  const [query, setQuery] = useState('');
  const [confirm, setConfirm] = useState<string | null>(null);

  useEffect(() => {
    const t = window.setTimeout(() => { api.listRuns(query || undefined, 200).then(setRuns).catch(e => notify(String(e))); }, 200);
    return () => window.clearTimeout(t);
  }, [query, notify]);

  async function remove(id: string) {
    if (confirm !== id) { setConfirm(id); return; }
    try { await api.deleteRun(id); setRuns(r => (r ?? []).filter(x => x.id !== id)); notify('Run deleted.'); } catch (e) { notify(String(e)); }
    setConfirm(null);
  }

  return <div className="rt-page">
    <h1 className="rt-title">History</h1>
    <label className="rt-search"><Search size={15}/><input className="rt-input" value={query} onChange={e => setQuery(e.target.value)} placeholder="Search questions and answers" aria-label="Search history"/></label>
    {runs && runs.length === 0 && <p className="rt-hint">{query ? 'Nothing matches.' : 'No runs yet. Ask a question to start.'}</p>}
    <div className="rt-list">
      {(runs ?? []).map(r => <div key={r.id} className="rt-list-row" role="button" tabIndex={0} onClick={() => onOpen(r.id)} onKeyDown={e => { if (e.key === 'Enter') onOpen(r.id); }}>
        <div className="rt-list-main"><strong>{r.question}</strong><small>{new Date(r.createdAt).toLocaleString()} · {r.seatCount} agents · ${r.settledUsd}</small></div>
        <span className={'rt-badge ' + r.state}>{LABEL[r.state] ?? 'Running'}</span>
        <button className={'rt-icon' + (confirm === r.id ? ' danger' : '')} aria-label={confirm === r.id ? 'Confirm delete' : 'Delete run'} title={confirm === r.id ? 'Click again to delete' : 'Delete'} onClick={e => { e.stopPropagation(); void remove(r.id); }}>{confirm === r.id ? 'Delete?' : <Trash2 size={15}/>}</button>
      </div>)}
    </div>
  </div>;
}
