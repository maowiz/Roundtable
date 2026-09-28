// One run, start to finish, on one screen: a four-step progress bar, the final answer when it
// exists, and every paper of the chosen step underneath.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ArrowLeft, Check, Copy, Square, RotateCw, X } from 'lucide-react';
import { api, attachRun, type RunSnapshot } from '../api/client';
import { RealPaperInspector, seatModelFor } from '../desktop';
import StageDocs, { type LivePaper } from '../live/StageDocs';
import { Markdown } from '../live/markdown';

const STEPS = [
  { stage: 'answer', title: 'Answers' },
  { stage: 'review', title: 'Reviews' },
  { stage: 'improve', title: 'Improve' },
  { stage: 'synthesize', title: 'Final answer' },
] as const;
const ACTIVE = new Set(['validating', 'answering', 'reviewing', 'improving', 'synthesizing', 'cancelling']);

function clock(ms: number) {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

function tokens(n: number) {
  return n >= 10_000 ? `${(n / 1000).toFixed(1)}k` : n.toLocaleString();
}

function statusLine(state: string, stageTitle: string) {
  if (state === 'completed') return 'Completed';
  if (state === 'cancelled') return 'Stopped';
  if (state === 'failed') return 'Failed';
  if (state === 'blocked') return 'Paused';
  if (state === 'interrupted') return 'Paused: the app was closed';
  if (state === 'cancelling') return 'Stopping…';
  return 'Running · ' + stageTitle;
}

export default function RunView({ runId, notify, onNew }: { runId: string; notify: (m: string) => void; onNew: () => void }) {
  const [snap, setSnap] = useState<RunSnapshot | null>(null);
  const [papers, setPapers] = useState<LivePaper[]>([]);
  const [chosen, setChosen] = useState<number | null>(null);
  const [inspect, setInspect] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());
  const [busy, setBusy] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const finalRef = useRef<HTMLElement>(null);

  const refresh = useCallback(() => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      api.getRunSnapshot(runId).then(setSnap).catch(() => {});
      api.listRunPapers(runId).then(p => setPapers(p as LivePaper[])).catch(() => {});
    }, 250);
  }, [runId]);

  useEffect(() => {
    let off: (() => void) | undefined;
    let gone = false;
    setSnap(null); setPapers([]); setChosen(null);
    attachRun(runId, () => refresh(), s => setSnap(s)).then(u => { if (gone) u(); else off = u; }).catch(e => notify(String(e)));
    api.listRunPapers(runId).then(p => setPapers(p as LivePaper[])).catch(() => {});
    return () => { gone = true; off?.(); window.clearTimeout(timer.current); };
  }, [runId, refresh, notify]);

  const active = !!snap && ACTIVE.has(snap.run.state);
  useEffect(() => { if (!active) return; const t = window.setInterval(() => setNow(Date.now()), 1000); return () => window.clearInterval(t); }, [active]);
  // cost and tokens come from the ledger; refresh them on a short timer as well as on every event
  useEffect(() => { if (!active) return; const t = window.setInterval(refresh, 2500); return () => window.clearInterval(t); }, [active, refresh]);

  const seats = useMemo(() => {
    const list = snap?.seats ?? [];
    const gateway = new Set(list.map(s => s.provider)).size < list.length;
    return list.map((s, i) => seatModelFor(s.label, s.provider, s.modelId, i, gateway));
  }, [snap]);

  if (!snap) return <div className="rt-page"><p className="rt-hint">Loading the run…</p></div>;

  const run = snap.run;
  const currentIdx = Math.max(0, STEPS.findIndex(s => s.stage === run.stage));
  const cov = (i: number) => snap.coverage.find(c => c.stage === STEPS[i].stage) ?? { planned: 0, accepted: 0 };
  const done = snap.coverage.reduce((a, c) => a + c.accepted, 0);
  const total = snap.coverage.reduce((a, c) => a + c.planned, 0);
  const shown = chosen ?? (run.state === 'completed' ? 0 : currentIdx);
  const final = snap.finalPaper as Record<string, any> | null;
  const finalSources = papers.find(p => p.kind === 'final')?.sources ?? [];
  const uncertain = snap.calls.filter(c => c.state === 'uncertain');
  const started = Date.parse(run.createdAt);
  const ended = run.finishedAt ? Date.parse(run.finishedAt) : now;

  async function act(fn: () => Promise<unknown>, ok: string) {
    setBusy(true);
    try { await fn(); notify(ok); refresh(); } catch (e) { notify(String(e)); } finally { setBusy(false); }
  }

  return <div className="rt-page">
    <button className="rt-link rt-back" onClick={onNew}><ArrowLeft size={14}/> New question</button>
    <header className="rt-run-head">
      <p className={'rt-eyebrow ' + (active ? 'live' : '')}>{active && <span className="rt-pulse"/>}{statusLine(run.state, STEPS[currentIdx].title)}</p>
      <h1 className="rt-run-q">{run.question}</h1>
      <div className="rt-meters">
        <span><b>${run.settledUsd}</b> spent{run.unresolvedUsd !== '0.00' ? <em> + ${run.unresolvedUsd} unconfirmed</em> : null}</span>
        <span><b>{tokens(snap.usage?.inputTokens ?? 0)}</b> tokens in</span>
        <span><b>{tokens(snap.usage?.outputTokens ?? 0)}</b> tokens out{(snap.usage?.reasoningTokens ?? 0) > 0 ? <em> incl. {tokens(snap.usage.reasoningTokens)} thinking</em> : null}</span>
        <span><b>{done}</b> of {total} calls</span>
        <span><b>{clock(ended - started)}</b></span>
      </div>
      {active && (snap.inFlight ?? []).length > 0 && <div className="rt-waiting">
        <span className="rt-pulse"/>
        <span>Waiting for {snap.inFlight.map((f, i) => <b key={i}>{f.stage === 'synthesize' ? 'Final writer' : (snap.seats[f.seat]?.label ?? 'agent ' + (f.seat + 1))} · {STEPS.find(s => s.stage === f.stage)?.title} · {clock(now - Date.parse(f.since))}{f.attempt > 1 ? ` (try ${f.attempt})` : ''}{i < snap.inFlight.length - 1 ? ', ' : ''}</b>)}</span>
        <em>Thinking models can take a few minutes per reply. Nothing is stuck while this timer moves.</em>
      </div>}
      <p className="rt-hint">Cost is what the provider charged for each call, updated as calls finish.</p>
      {(run.state === 'blocked' || run.state === 'interrupted' || run.state === 'failed') && run.reasonText && <p className="rt-warn">{run.reasonText}</p>}
      <div className="rt-row">
        {active && run.state !== 'cancelling' && <button className="rt-btn" disabled={busy} onClick={() => act(() => api.cancelRun(runId), 'Stopping. Calls already in flight will finish and be counted.')}><Square size={14}/> Stop</button>}
        {(run.state === 'blocked' || run.state === 'interrupted') && (uncertain.length > 0
          ? <button className="rt-btn primary" disabled={busy} onClick={() => act(async () => { for (const c of uncertain) await api.retryUncertain(c.id); await api.resumeRun(runId); }, 'Retrying. Those calls may be billed again.')}><RotateCw size={14}/> Retry {uncertain.length} unanswered call{uncertain.length === 1 ? '' : 's'}</button>
          : <button className="rt-btn primary" disabled={busy} onClick={() => act(() => api.resumeRun(runId), 'Resuming. Nothing that succeeded is sent again.')}><RotateCw size={14}/> Resume</button>)}
      </div>
    </header>

    {final && <section className="rt-card rt-final" ref={finalRef}>
      <div className="rt-final-head"><h2>Final answer</h2><button className="rt-btn ghost" onClick={() => navigator.clipboard.writeText(String(final.answer ?? '')).then(() => notify('Answer copied.'))}><Copy size={14}/> Copy</button></div>
      <Markdown text={String(final.answer ?? '')}/>
      {Array.isArray(final.key_claims) && final.key_claims.length > 0 && <div className="rt-final-block"><p className="rt-eyebrow">KEY CLAIMS</p><ul>{final.key_claims.map((k: any, i: number) => <li key={i}>{k.text} <em className={'support ' + k.support}>{k.support === 'supported' ? `supported by ${(k.supporting ?? []).length}` : k.support}</em></li>)}</ul></div>}
      {Array.isArray(final.unresolved_conflicts) && final.unresolved_conflicts.length > 0 && <div className="rt-final-block"><p className="rt-eyebrow">STILL DISPUTED</p><ul>{final.unresolved_conflicts.map((u: string, i: number) => <li key={i}>{u}</li>)}</ul></div>}
      {finalSources.length > 0 && <div className="rt-final-block"><p className="rt-eyebrow">WEB SOURCES</p><ol>{finalSources.map((s, i) => <li key={i}>{s.title || s.url} <small>{s.url}</small></li>)}</ol></div>}
    </section>}

    <section className="rt-section">
      <div className="rt-section-head"><h2>{final ? 'How the agents got there' : 'Progress'}</h2><small>Click a step to read its papers.</small></div>
      <div className="rt-steps">
        {STEPS.map((s, i) => {
          const c = cov(i);
          const state = c.planned > 0 && c.accepted >= c.planned ? 'done' : i === currentIdx && active ? 'now' : '';
          return <button key={s.stage} className={'rt-step ' + state + (shown === i ? ' shown' : '')} onClick={() => setChosen(i)} aria-pressed={shown === i}>
            <span className="rt-step-dot">{state === 'done' ? <Check size={13}/> : i + 1}</span>
            <span className="rt-step-text"><strong>{s.title}</strong><small>{c.accepted} of {c.planned}</small></span>
          </button>;
        })}
      </div>
      <StageDocs stage={shown} seats={seats} papers={papers} planned={cov(shown).planned} onStage={i => setChosen(i)} onInspect={setInspect} finalReady={!!final} onOpenFinal={() => finalRef.current?.scrollIntoView({ behavior: 'smooth' })}/>
    </section>

    {inspect && <div className="rt-drawer-backdrop" onMouseDown={e => { if (e.target === e.currentTarget) setInspect(null); }}>
      <aside className="rt-drawer wide" role="dialog" aria-label="Paper details">
        <header className="rt-drawer-head"><div><p className="rt-eyebrow">PAPER</p><h2>What was sent and received</h2></div><button className="rt-icon" onClick={() => setInspect(null)} aria-label="Close"><X size={18}/></button></header>
        <div className="rt-drawer-body"><RealPaperInspector paperId={inspect} notify={notify}/></div>
      </aside>
    </div>}
  </div>;
}
