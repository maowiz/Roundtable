// The home screen: type a question, see your agents, press Start.
// Agents are cards: "+" adds one, "×" removes one, clicking a card opens everything about it.

import { useEffect, useRef, useState } from 'react';
import { ArrowRight, Globe, KeyRound, Paperclip, Plus, Sparkles, X } from 'lucide-react';
import { api, WEB_SEARCH_OFF, type PanelDoc, type PreflightReport, type WebSearchDoc } from '../api/client';
import { seatModelFor } from '../desktop';
import AgentEditor from './AgentEditor';
import { MAX_AGENTS, MIN_AGENTS, newAgent, type TeamApi } from './state';

const SEARCH_STAGES: { key: keyof WebSearchDoc; label: string }[] = [
  { key: 'answer', label: 'Answers' }, { key: 'review', label: 'Reviews' }, { key: 'improve', label: 'Improve' }, { key: 'synthesize', label: 'Final' },
];

function ConnectCard({ notify, onSaved }: { notify: (m: string) => void; onSaved: () => void }) {
  const [key, setKey] = useState('');
  const [busy, setBusy] = useState(false);
  async function save() {
    if (!key.trim()) return;
    setBusy(true);
    try { await api.saveCredential('openrouter', 'OpenRouter', key.trim()); setKey(''); notify('Key saved in Windows Credential Manager.'); onSaved(); }
    catch (e) { notify(String(e)); }
    finally { setBusy(false); }
  }
  return <section className="rt-card rt-connect">
    <KeyRound size={22}/>
    <div>
      <h2>Connect OpenRouter to start</h2>
      <p className="rt-hint">Paste your OpenRouter API key once. It is stored in Windows Credential Manager, never in a file. Other providers can be added in Settings.</p>
      <div className="rt-row"><input className="rt-input" type="password" autoComplete="off" placeholder="sk-or-…" value={key} onChange={e => setKey(e.target.value)} onKeyDown={e => { if (e.key === 'Enter') void save(); }} aria-label="OpenRouter API key"/><button className="rt-btn primary" disabled={busy || !key.trim()} onClick={save}>Save key</button></div>
    </div>
  </section>;
}

export default function AskScreen({ teamApi, ceilingUsd, setCeilingUsd, notify, toast, onStarted }: {
  teamApi: TeamApi;
  ceilingUsd: string;
  setCeilingUsd: (v: string) => void;
  notify: (m: string) => void;
  toast: (m: string, action?: { label: string; run: () => void }) => void;
  onStarted: (runId: string) => void;
}) {
  const { team, options, status, savedVersion, update, flush, reload, defaults } = teamApi;
  const [question, setQuestion] = useState(() => localStorage.getItem('rt-draft') ?? '');
  const [context, setContext] = useState('');
  const [showContext, setShowContext] = useState(false);
  const [editing, setEditing] = useState<{ index: number | 'writer'; fresh: boolean } | null>(null);
  const [report, setReport] = useState<PreflightReport | null>(null);
  const [starting, setStarting] = useState(false);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => { localStorage.setItem('rt-draft', question); }, [question]);

  // cost estimate and readiness, refreshed after every saved change
  useEffect(() => {
    if (!team?.id) { setReport(null); return; }
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      api.preflightRun({ question: question || ' ', referenceText: context || null, panelId: team.id!, ceilingUsd })
        .then(setReport).catch(e => setReport({ ok: false, seatCount: 0, baseCalls: 0, maxAttempts: 0, estimateLowUsd: '0.00', estimateHighUsd: '0.00', unverifiedPrices: [], problems: [String(e)], warnings: [], webSearchCalls: 0, webSearchFeeUsd: '0.00' }));
    }, 350);
  }, [team?.id, savedVersion, question, context, ceilingUsd]);

  if (status === 'loading') return <div className="rt-page"><p className="rt-hint">Loading…</p></div>;
  if (status === 'needs-key' || !team) {
    return <div className="rt-page">
      <h1 className="rt-title">Ask your agents</h1>
      {status === 'no-models' ? <section className="rt-card"><h2>No usable models yet</h2><p className="rt-hint">The connected provider has no models with a verified price. Add an OpenRouter key in Settings.</p></section>
        : <ConnectCard notify={notify} onSaved={() => void reload()}/>}
    </div>;
  }

  const seats = team.seats;
  const gateway = new Set(seats.map(s => s.provider)).size < seats.length;
  const models = seats.map((s, i) => seatModelFor(s.label, s.provider, s.modelId, i, gateway));
  const ws = team.webSearch ?? WEB_SEARCH_OFF;

  function addAgent() {
    if (!team || team.seats.length >= MAX_AGENTS) return;
    const usable = options.filter(o => o.verified);
    if (usable.length === 0) return notify('Connect a provider in Settings first.');
    const used = new Set(team.seats.map(s => s.provider + '|' + s.modelId));
    const pick = usable.find(o => !used.has(o.key)) ?? usable[0];
    const words = team.seats[0]?.answer.maxOutputWords ?? 700;
    const next: PanelDoc = { ...team, seats: [...team.seats, newAgent(pick, team.seats.map(s => s.label), words, defaults)] };
    update(next);
    setEditing({ index: next.seats.length - 1, fresh: true });
  }

  function removeAgent(i: number) {
    if (!team) return;
    if (team.seats.length <= MIN_AGENTS) return notify('Keep at least two agents: they review each other.');
    const before = team;
    const gone = team.seats[i].label;
    update({ ...team, seats: team.seats.filter((_, j) => j !== i) });
    setEditing(null);
    toast(`${gone} removed.`, { label: 'Undo', run: () => update(before) });
  }

  async function start() {
    if (!team?.id || !question.trim()) return;
    setStarting(true);
    try {
      await flush();
      const runId = await api.startRun({ question: question.trim(), referenceText: context.trim() || null, panelId: team.id, ceilingUsd });
      setQuestion('');
      onStarted(runId);
    } catch (e) { notify(String(e)); }
    finally { setStarting(false); }
  }

  const problems = (report?.problems ?? []).filter(p => !p.startsWith('Add a question'));
  const canStart = !!question.trim() && !!report?.ok && !starting;

  return <div className="rt-page">
    <h1 className="rt-title">Ask your agents</h1>
    <p className="rt-sub">Each agent answers alone, reviews the others without knowing who wrote what, improves its own answer, then one writer combines them.</p>

    <section className="rt-card rt-ask">
      <textarea className="rt-question" value={question} onChange={e => setQuestion(e.target.value)} placeholder="What should your agents think about?" maxLength={12000} aria-label="Question" onKeyDown={e => { if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && canStart) void start(); }}/>
      {showContext && <textarea className="rt-input rt-area" rows={4} value={context} onChange={e => setContext(e.target.value)} placeholder="Background, notes or source text the agents should use" maxLength={20000} aria-label="Reference text"/>}
      <button className="rt-link" onClick={() => setShowContext(v => !v)}><Paperclip size={13}/> {showContext ? 'Remove reference text' : 'Add reference text'}</button>
    </section>

    <section className="rt-section">
      <div className="rt-section-head"><h2>Agents <span className="rt-count">{seats.length}</span></h2><small>Click an agent to change its name, model or system prompt.</small></div>
      <div className="rt-agents">
        {seats.map((s, i) => <div key={i} className="rt-agent" role="button" tabIndex={0} onClick={() => setEditing({ index: i, fresh: false })} onKeyDown={e => { if (e.key === 'Enter') setEditing({ index: i, fresh: false }); }} aria-label={'Edit ' + s.label}>
          <span className={'avatar ' + models[i].color}>{models[i].letter}</span>
          <div className="rt-agent-text"><strong>{s.label || 'Unnamed'}</strong><small>{s.modelId}</small></div>
          <button className="rt-x" aria-label={'Delete ' + s.label} disabled={seats.length <= MIN_AGENTS} title={seats.length <= MIN_AGENTS ? 'Keep at least two agents' : 'Delete'} onClick={e => { e.stopPropagation(); removeAgent(i); }}><X size={14}/></button>
        </div>)}
        {seats.length < MAX_AGENTS && <button className="rt-agent rt-add" onClick={addAgent} aria-label="Add agent"><Plus size={20}/><span>Add agent</span></button>}
      </div>
      <div className="rt-writer" role="button" tabIndex={0} onClick={() => setEditing({ index: 'writer', fresh: false })} onKeyDown={e => { if (e.key === 'Enter') setEditing({ index: 'writer', fresh: false }); }} aria-label="Edit final writer">
        <span className="avatar violet"><Sparkles size={15}/></span>
        <div className="rt-agent-text"><strong>Final writer</strong><small>{team.synthesizer.modelId}</small></div>
        <span className="rt-hint">combines the improved answers into one</span>
      </div>
    </section>

    <section className="rt-runbar">
      <div className="rt-runbar-opts">
        <label className="rt-inline">Spending limit $<input className="rt-input rt-small" value={ceilingUsd} onChange={e => setCeilingUsd(e.target.value)} aria-label="Spending limit"/></label>
        <span className="rt-inline"><Globe size={14}/> Web search:
          {SEARCH_STAGES.map(s => <button key={s.key} className={'rt-chip' + (ws[s.key] ? ' on' : '')} aria-pressed={ws[s.key]} onClick={() => update({ ...team, webSearch: { ...ws, [s.key]: !ws[s.key] } })}>{s.label}</button>)}
        </span>
      </div>
      <div className="rt-runbar-go">
        <div className="rt-estimate">
          {report && <span>{report.baseCalls} calls · about ${report.estimateLowUsd}–${report.estimateHighUsd}</span>}
          {problems.map((p, i) => <span key={i} className="rt-error">{p}</span>)}
        </div>
        <button className="rt-btn primary big" disabled={!canStart} onClick={start}>{starting ? 'Starting…' : 'Start'} <ArrowRight size={17}/></button>
      </div>
    </section>

    {editing && <AgentEditor team={team} index={editing.index} options={options} defaults={defaults} focusName={editing.fresh} onChange={update} onDelete={removeAgent} onClose={() => setEditing(null)}/>}
  </div>;
}
