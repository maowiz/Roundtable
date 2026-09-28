// Stage document browser: the four stage boxes open these pages. Every paper shown here is a
// committed record from the ledger, labelled with who wrote it and, for reviews, whose answer it is about.
// Cards open and close in place; the page itself scrolls, nothing inside it does.

import { useEffect, useState } from 'react';
import { ArrowLeft, ArrowRight, ArrowUpRight, CheckCircle2, ChevronDown, CircleAlert, Globe, Sparkles } from 'lucide-react';
import type { SeatModel } from './Roundtable';
import { Markdown, excerpt } from './markdown';

export interface LivePaper { id: string; kind: string; authorSeat: number; targetSeat: number | null; body: any; sources?: { url: string; title: string }[] }

const STAGE_TITLES = ['Independent answers', 'Peer reviews', 'Self-improvement', 'Final synthesis'];
const STAGE_BLURB = [
  'Each seat answered alone. None of them had seen another answer.',
  'Every answer was reviewed by every other seat in a fresh session. A reviewer saw one answer and nothing else.',
  'Each seat received only its own answer and the reviews written about it, then wrote an improved version.',
  'The final writer received the improved answers in random order and wrote one considered answer.',
];

interface Props { stage: number; seats: SeatModel[]; papers: LivePaper[]; planned: number; onStage: (i: number) => void; onInspect: (paperId: string) => void; finalReady: boolean; onOpenFinal: () => void }

function Avatar({ m }: { m?: SeatModel }) {
  return <span className={'avatar small ' + (m?.color ?? 'slate')}>{m?.letter ?? '?'}</span>;
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return <div className="doc-section"><span className="eyebrow">{title}</span>{children}</div>;
}

/** One collapsible paper. `summary` is the one-line preview shown while closed. */
function Sources({ list }: { list?: { url: string; title: string }[] }) {
  if (!list || list.length === 0) return null;
  return <div className="doc-section doc-sources"><span className="eyebrow"><Globe size={11}/> WEB SOURCES THIS SEAT SAW</span><ol>{list.map((s, i) => <li key={i}><span>{s.title || s.url}</span><small>{s.url}</small></li>)}</ol></div>;
}

function Card({ id, open, onToggle, avatar, title, sub, summary, children, extra, sources }: { id: string; open: boolean; onToggle: (id: string) => void; avatar: React.ReactNode; title: string; sub: string; summary: string; children: React.ReactNode; extra?: React.ReactNode; sources?: { url: string; title: string }[] }) {
  return <article className={'doc-card' + (open ? ' open' : '')}>
    <button className="doc-head" onClick={() => onToggle(id)} aria-expanded={open}>
      {avatar}
      <div className="doc-title"><strong>{title}</strong><small>{sub}{sources && sources.length > 0 && <> · <Globe size={11}/> searched the web</>}</small></div>
      <span className="doc-toggle"><span>{open ? 'Close' : 'Read'}</span><ChevronDown size={15}/></span>
    </button>
    {!open && <p className="doc-summary">{summary}</p>}
    {open && <div className="doc-body">{children}<Sources list={sources}/><footer>{extra}</footer></div>}
  </article>;
}

export default function StageDocs({ stage, seats, papers, planned, onStage, onInspect, finalReady, onOpenFinal }: Props) {
  const name = (i: number) => seats[i]?.name ?? `Seat ${i + 1}`;
  const model = (i: number) => seats[i]?.role ?? '';
  const answers = papers.filter(p => p.kind === 'answer').sort((a, b) => a.authorSeat - b.authorSeat);
  const reviews = papers.filter(p => p.kind === 'review');
  const improved = papers.filter(p => p.kind === 'improved').sort((a, b) => a.authorSeat - b.authorSeat);
  const final = papers.find(p => p.kind === 'final');
  const have = [answers.length, reviews.length, improved.length, final ? 1 : 0][stage];
  const ids = [answers, reviews, improved, final ? [final] : []][stage].map(p => p.id);

  // which cards are open; the final answer opens by itself, everything else starts closed
  const [open, setOpen] = useState<Set<string>>(new Set());
  useEffect(() => { setOpen(new Set(stage === 3 && final ? [final.id] : [])); }, [stage, final?.id]);
  const toggle = (id: string) => setOpen(prev => { const n = new Set(prev); if (n.has(id)) n.delete(id); else n.add(id); return n; });
  const allOpen = ids.length > 0 && ids.every(id => open.has(id));

  const inspect = (id: string) => <button className="text-button doc-open" onClick={() => onInspect(id)}>Envelope, views and attempts <ArrowUpRight size={12}/></button>;

  const body = () => {
    if (stage === 0) return <div className="doc-list-col">{answers.map(p => <Card key={p.id} id={p.id} open={open.has(p.id)} onToggle={toggle} avatar={<Avatar m={seats[p.authorSeat]}/>} title={name(p.authorSeat)} sub={model(p.authorSeat) + ' · independent answer'} summary={excerpt(p.body.answer)} extra={inspect(p.id)} sources={p.sources}>
      <Markdown text={p.body.answer}/>
      {Array.isArray(p.body.claims) && p.body.claims.length > 0 && <Section title="CLAIMS"><ul className="plain">{p.body.claims.map((c: any) => <li key={c.id}><b>{c.id}</b> {c.text}</li>)}</ul></Section>}
      {Array.isArray(p.body.assumptions) && p.body.assumptions.length > 0 && <Section title="ASSUMPTIONS"><ul>{p.body.assumptions.map((a: string, i: number) => <li key={i}>{a}</li>)}</ul></Section>}
      {Array.isArray(p.body.limitations) && p.body.limitations.length > 0 && <Section title="LIMITATIONS"><ul>{p.body.limitations.map((a: string, i: number) => <li key={i}>{a}</li>)}</ul></Section>}
    </Card>)}</div>;

    if (stage === 1) return <div className="doc-groups">{seats.map((s, t) => {
      const rs = reviews.filter(r => r.targetSeat === t).sort((a, b) => a.authorSeat - b.authorSeat);
      return <section key={t} className="doc-group">
        <h3><Avatar m={s}/> Reviews of {name(t)}’s answer <small>{rs.length} of {seats.length - 1}</small></h3>
        <div className="doc-list-col">{rs.map(r => {
          const issues: any[] = Array.isArray(r.body.issues) ? r.body.issues : [];
          const ok: string[] = Array.isArray(r.body.correct_claim_ids) ? r.body.correct_claim_ids : [];
          const summary = [ok.length ? `judged ${ok.length} claim${ok.length === 1 ? '' : 's'} correct` : '', issues.length ? `${issues.length} issue${issues.length === 1 ? '' : 's'}` : 'no issues', Array.isArray(r.body.missing) && r.body.missing.length ? `${r.body.missing.length} things missing` : ''].filter(Boolean).join(' · ');
          return <Card key={r.id} id={r.id} open={open.has(r.id)} onToggle={toggle} avatar={<Avatar m={seats[r.authorSeat]}/>} title={`Written by ${name(r.authorSeat)}`} sub={`about ${name(t)}’s answer · saw only that answer`} summary={summary.charAt(0).toUpperCase() + summary.slice(1) + '.'} extra={inspect(r.id)} sources={r.sources}>
            {ok.length > 0 && <p className="doc-ok"><CheckCircle2 size={14}/> Judged correct: {ok.join(', ')}</p>}
            {issues.length === 0 && <p className="doc-ok"><CheckCircle2 size={14}/> No material issue found.</p>}
            {issues.map((i: any, idx: number) => <div key={i.id} className="doc-issue"><span className="sev">{idx + 1}</span><div><b>{i.target_claim_id ? `On ${i.target_claim_id}` : 'General'}</b> {i.explanation}{i.suggested_correction && <em>Suggested: {i.suggested_correction}</em>}</div></div>)}
            {Array.isArray(r.body.missing) && r.body.missing.length > 0 && <Section title="MISSING"><ul>{r.body.missing.map((m: string, i: number) => <li key={i}>{m}</li>)}</ul></Section>}
            {r.body.needs_external_verification && <p className="doc-warn"><CircleAlert size={14}/> The reviewer asked for external verification.</p>}
          </Card>;
        })}{rs.length === 0 && <p className="muted">No review yet.</p>}</div>
      </section>;
    })}</div>;

    if (stage === 2) return <div className="doc-list-col">{improved.map(p => {
      const d: any[] = Array.isArray(p.body.decisions) ? p.body.decisions : [];
      const count = (k: string) => d.filter(x => x.decision === k).length;
      return <Card key={p.id} id={p.id} open={open.has(p.id)} onToggle={toggle} avatar={<Avatar m={seats[p.authorSeat]}/>} title={name(p.authorSeat)} sub={`improved answer · accepted ${count('accepted')}, partly ${count('partly_accepted')}, rejected ${count('rejected')} of ${d.length} review points`} summary={excerpt(p.body.answer)} extra={inspect(p.id)} sources={p.sources}>
        <Markdown text={p.body.answer}/>
        {d.length > 0 && <Section title="HOW IT USED THE REVIEWS"><ul>{d.map((x: any, i: number) => <li key={i}><b>{String(x.decision).replace('_', ' ')}</b> {x.issue_id}: {x.reason}</li>)}</ul></Section>}
        {Array.isArray(p.body.unresolved) && p.body.unresolved.length > 0 && <Section title="STILL UNRESOLVED"><ul>{p.body.unresolved.map((u: string, i: number) => <li key={i}>{u}</li>)}</ul></Section>}
      </Card>;
    })}</div>;

    if (!final) return null;
    return <div className="doc-list-col"><Card id={final.id} open={open.has(final.id)} onToggle={toggle} avatar={<span className="avatar small violet"><Sparkles size={14}/></span>} title="The considered answer" sub={`written by the final writer from ${improved.length} improved answers`} summary={excerpt(final.body.answer)} sources={final.sources} extra={<>{inspect(final.id)}{finalReady && <button className="primary small" onClick={onOpenFinal}>Open the final answer page <ArrowRight size={13}/></button>}</>}>
      <Markdown text={final.body.answer}/>
      {Array.isArray(final.body.key_claims) && <Section title="KEY CLAIMS"><ul>{final.body.key_claims.map((k: any, i: number) => <li key={i}>{k.text} <em className={'support ' + k.support}>{k.support === 'supported' ? `supported by ${(k.supporting ?? []).length}` : k.support}</em></li>)}</ul></Section>}
      {Array.isArray(final.body.unresolved_conflicts) && final.body.unresolved_conflicts.length > 0 && <Section title="UNRESOLVED CONFLICTS"><ul>{final.body.unresolved_conflicts.map((u: string, i: number) => <li key={i}>{u}</li>)}</ul></Section>}
    </Card></div>;
  };

  return <div className="stage-docs">
    <div className="stage-docs-head">
      <div><p className="eyebrow">STAGE {stage + 1} OF 4</p><h2>{STAGE_TITLES[stage]}</h2><p className="muted">{STAGE_BLURB[stage]}</p></div>
      <div className="stage-docs-tools"><span className="doc-count">{have} of {planned} ready</span>{ids.length > 1 && <button className="text-button" onClick={() => setOpen(allOpen ? new Set() : new Set(ids))}>{allOpen ? 'Close all' : 'Open all'}</button>}</div>
    </div>
    {have === 0 ? <div className="doc-empty"><p>Nothing here yet. This stage {stage === 0 ? 'starts first' : 'starts after the previous one completes'}.</p></div> : body()}
    <div className="stage-nav">
      <button className="secondary" disabled={stage === 0} onClick={() => onStage(stage - 1)}><ArrowLeft size={14}/> {stage > 0 ? STAGE_TITLES[stage - 1] : 'Start'}</button>
      <span className="stage-nav-dots">{STAGE_TITLES.map((s, i) => <i key={s} className={i === stage ? 'on' : ''} title={s}/>)}</span>
      <button className="secondary" disabled={stage === 3} onClick={() => onStage(stage + 1)}>{stage < 3 ? STAGE_TITLES[stage + 1] : 'End'} <ArrowRight size={14}/></button>
    </div>
  </div>;
}
