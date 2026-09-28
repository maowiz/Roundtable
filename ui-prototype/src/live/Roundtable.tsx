import { useEffect, useMemo, useRef, useState } from 'react';
import { ArrowRight, ArrowUpRight, Check, ChevronLeft, FileText, Globe, LayoutGrid, List, LockKeyhole, Mail, Minus, Pause, Play, Plus, Search, ShieldCheck, Sparkles, X } from 'lucide-react';
import { buildScript, derive, easeInOut, easeOut, clamp01, type Script, type World } from './script';
import StageDocs, { type LivePaper } from './StageDocs';

export interface SeatModel { name: string; provider: string; letter: string; color: string; role: string }
export const seatHex: Record<string, string> = { clay: '#c48a72', sage: '#7fa08f', blue: '#6f93cc', violet: '#9a8bb8', teal: '#4fa39d', amber: '#c9a054', rose: '#c67f92', slate: '#8a97a8' };

/// A real run feeding the table (desktop). The script grows as engine events arrive.
export interface LiveProps {
  script: Script; now: number; state: string; reason: string | null; settledUsd: string; unresolvedUsd: string;
  active: boolean; finalReady: boolean; uncertainCount: number;
  /** Accepted calls so far and the total planned for the run. */
  done: number; total: number; stageLabel: string;
  /** Every accepted paper with its body, for the stage browser. */
  papers: LivePaper[];
  onInspectPaper: (paperId: string) => void;
  elapsedLabel: string;
  /** Which of the four stages ran with provider-side web search. */
  webSearch: [boolean, boolean, boolean, boolean];
  onCancel: () => void; onResume: () => void; onRetryUncertain: () => void; onInspectSeat: (seat: number) => void;
}

interface Props {
  models: SeatModel[]; seatIds: number[]; question: string; points: string[][];
  onInspect: (modelId: number) => void; onResult: () => void; reducedMotion: boolean; runKey: number; startAtEnd?: boolean;
  onAddSeat: () => void; onRemoveSeat: (modelId: number) => void; maxSeats: number; notify: (m: string) => void;
  live?: LiveProps;
}

const stageNames = ['Independent answers', 'Peer reviews', 'Self-improvement', 'Final synthesis'];
const stageEyebrow = ['THINK INDEPENDENTLY', 'ROTATE & REVIEW', 'REFLECT & IMPROVE', 'BRING IT TOGETHER'];
const stageCopy = [
  'Each AI answers the original question without seeing another response.',
  'Each AI moves one chair and reviews the answer it finds there, with a clean context each time.',
  'Each AI returns home and receives only its own answer and the reviews written about it.',
  'The improved answers are shuffled, then the final writer brings them into one considered response.',
];

function useSize(ref: React.RefObject<HTMLElement | null>, key: unknown) {
  const [size, setSize] = useState({ w: 900, h: 540 });
  useEffect(() => {
    const el = ref.current; if (!el) return;
    const read = (w: number, h: number) => { if (w > 50 && h > 50) setSize({ w, h }); };
    read(el.clientWidth, el.clientHeight);
    const ro = new ResizeObserver(([e]) => read(e.contentRect.width, e.contentRect.height));
    ro.observe(el); return () => ro.disconnect();
  }, [ref, key]);
  return size;
}

const fmt = (ms: number) => { const s = Math.floor(ms / 1000); return Math.floor(s / 60) + ':' + String(s % 60).padStart(2, '0'); };

export default function Roundtable({ models, seatIds, question, points, onInspect, onResult, reducedMotion, runKey, startAtEnd = false, onAddSeat, onRemoveSeat, maxSeats, notify, live }: Props) {
  const n = seatIds.length;
  const seats = seatIds.map(id => models[id]);
  const [cinematic, setCinematic] = useState(true);
  const demoScript: Script = useMemo(() => buildScript(n, cinematic), [n, cinematic]);
  const script: Script = live ? live.script : demoScript;
  const [follow, setFollow] = useState(true);
  const [t, setT] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(1);
  const [view, setView] = useState<'table' | 'matrix' | 'list' | 'stage'>('table');
  const [stageView, setStageView] = useState(0);
  const [selected, setSelected] = useState<number | null>(null);
  const [hover, setHover] = useState<number | null>(null);
  const [cancelled, setCancelled] = useState(false);
  const tableRef = useRef<HTMLDivElement>(null);
  const { w, h } = useSize(tableRef, view);
  const tRef = useRef(0); tRef.current = t;

  // open at the end when arriving from the Final Answer screen
  useEffect(() => { if (startAtEnd) setT(script.total); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  // autoplay when a run starts from Home
  useEffect(() => { if (runKey > 0) { setT(0); setCancelled(false); setSelected(null); setPlaying(true); } }, [runKey]);
  // keep t inside the script when the seat count changes
  useEffect(() => { if (!live) setT(x => Math.min(x, script.total)); }, [script, live]);
  // live: the playhead follows the run unless the user scrubbed back
  useEffect(() => { if (live && follow) setT(live.now); }, [live, follow]);
  // player loop
  useEffect(() => {
    if (!playing || live) return;
    let raf = 0; let last = performance.now();
    const loop = (now: number) => {
      const dt = (now - last) * speed; last = now;
      const next = Math.min(script.total, tRef.current + dt);
      setT(next);
      if (next >= script.total) { setPlaying(false); return; }
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [playing, speed, script]);
  // keyboard
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || document.querySelector('dialog[open]')) return;
      if (e.key === 'Escape') setSelected(null);
    };
    addEventListener('keydown', onKey); return () => removeEventListener('keydown', onKey);
  });

  const world: World = useMemo(() => derive(script, t), [script, t]);
  const stage = world.stage;
  const started = t > 0;

  function toggle() {
    if (live) { setFollow(f => !f); return; }
    if (t >= script.total) setT(0); setCancelled(false); setPlaying(p => !p);
  }
  function jump(i: number) { setCancelled(false); const at = script.stageAt[i]; if (at > 0 || i === 0) setT(Math.min(at, script.total)); }
  function openStage(i: number) { setStageView(i); setView('stage'); setSelected(null); }
  const isPlaying = live ? live.active && follow : playing;
  // dev-only hook so screenshot scripts can position the sample without a visible timeline
  useEffect(() => { if (!live) { (window as any).__rtSeek = (ms: number) => { setPlaying(false); setT(ms); }; } }, [live]);

  // ----- geometry
  const cw = n <= 4 ? 176 : n <= 6 ? 150 : 120, ch = n <= 4 ? 118 : n <= 6 ? 102 : 84;
  const cx = w / 2, cy = h / 2;
  const Ry = Math.max(120, h / 2 - ch / 2 - 16);
  const Rx = Math.max(150, Math.min(w / 2 - cw / 2 - 16, Ry * 1.75));
  const R = Ry;
  const ang = (k: number) => ((-90 + (k * 360) / n) * Math.PI) / 180;
  const chairPos = (k: number) => ({ x: cx + Rx * Math.cos(ang(k)), y: cy + Ry * Math.sin(ang(k)) });
  const tokenRx = Math.max(90, Rx - cw / 2 - 30), tokenRy = Math.max(70, Ry - ch / 2 - 30);
  const tokenR = tokenRy;
  const tokenPos = (k: number) => ({ x: cx + tokenRx * Math.cos(ang(k)), y: cy + tokenRy * Math.sin(ang(k)) });
  const tokenK = (i: number) => {
    if (world.moveP >= 1) return i + world.rotation;
    if (reducedMotion) return i + world.moveFrom + (world.moveP < 0.5 ? 0 : 1);
    return i + world.moveFrom + easeInOut(world.moveP);
  };
  const curve = (a: { x: number; y: number }, b: { x: number; y: number }, p: number, bend = 0.18) => {
    const mx = (a.x + b.x) / 2, my = (a.y + b.y) / 2; const dx = b.x - a.x, dy = b.y - a.y;
    const c = { x: mx - dy * bend, y: my + dx * bend };
    const q = 1 - p; return { x: q * q * a.x + 2 * q * p * c.x + p * p * b.x, y: q * q * a.y + 2 * q * p * c.y + p * p * b.y };
  };
  const chairOf = (seat: number) => (seat + world.rotation) % n;

  // ----- derived copy
  const answered = world.think.filter(x => x >= 1).length;
  const improved = world.paper.filter(x => x === 'improved').length;
  const statusText = (i: number) => {
    const s = world.status[i];
    if (!started) return seats[i].role;
    if (s === 'thinking') return 'Thinking it through…';
    if (s === 'answered') return 'Answer ready';
    if (s === 'moving') return 'Moving one chair';
    if (s === 'seated') return 'Seated · waiting for an envelope';
    if (s === 'opening') return 'Opening a sealed envelope';
    if (s === 'reviewing') return 'Reviewing ' + seats[chairOf(i)].name + '’s answer';
    if (s === 'reviewed') return 'Review written';
    if (s === 'clearing') return 'Clearing context…';
    if (s === 'fresh') return 'Fresh context';
    if (s === 'returning') return 'Returning home';
    if (s === 'home') return 'Reading its reviews';
    if (s === 'improving') return 'Improving with ' + (n - 1) + ' reviews';
    if (s === 'improved') return stage === 3 ? 'Contribution included' : 'Improved answer ready';
    return seats[i].role;
  };
  const seatText = (i: number) => {
    const pt = points[i % points.length] || ['', ''];
    return `${seats[i].role}. ${pt[0]} ${pt[1]} I would test this with a small group first and expand only when the signal is clear.`;
  };
  const eventsSoFar = script.events.filter(e => e.t <= t && e.ev.type !== 'run_started').slice(-6).reverse();
  const describe = (e: (typeof script.events)[number]) => {
    const ev = e.ev;
    switch (ev.type) {
      case 'seat_thinking': return [seats[ev.seat].name, 'started an independent answer'];
      case 'paper_created': return ev.kind === 'final' ? ['Final writer', 'finished the considered answer'] : [seats[ev.seat].name, ev.kind === 'answer' ? 'finished an answer' : 'wrote an improved answer'];
      case 'rotation_started': return ['Everyone', 'moved one chair clockwise'];
      case 'envelope_sent': return [seats[ev.reviewer].name, 'received one sealed answer'];
      case 'review_started': return [seats[ev.reviewer].name, 'is reviewing ' + seats[ev.chair].name];
      case 'review_done': return [seats[ev.reviewer].name, 'reviewed ' + seats[ev.chair].name];
      case 'context_cleared': return [seats[ev.seat].name, 'context cleared'];
      case 'seats_returned': return ['Everyone', 'returned to their own chair'];
      case 'improve_started': return [seats[ev.seat].name, 'is reading its reviews'];
      case 'shuffle': return ['Controller', 'shuffled the improved answers'];
      case 'synthesis_started': return ['Final writer', 'started the synthesis'];
      case 'run_finished': return ['Roundtable', 'complete'];
      default: return ['', ''];
    }
  };

  // ----- centre stage
  const centre = () => {
    if (!started) return <div className="table-center"><span>READY WHEN YOU ARE</span><h2>{n} minds.{'\n'}One question.</h2><p>Press play, or add and remove seats first.</p></div>;
    if (stage === 0) return <div className="table-center"><span>{stageEyebrow[0]}</span><h2>One question.{'\n'}{n} perspectives.</h2><p>{answered} / {n} answers ready</p></div>;
    if (stage === 1) {
      const frac = world.reviewsDone / world.reviewsTotal; const r = 58, c = 2 * Math.PI * r;
      return <div className="table-center"><svg className="centre-ring" viewBox="0 0 140 140" aria-hidden="true"><circle cx="70" cy="70" r={r} className="ring-track"/><circle cx="70" cy="70" r={r} className="ring-fill" strokeDasharray={c} strokeDashoffset={c * (1 - frac)}/></svg><span>{stageEyebrow[1]}</span><h2>A fresh pair{'\n'}of eyes.</h2><p>Rotation {world.rotationIndex} of {n - 1} · {world.reviewsDone} / {world.reviewsTotal} reviews</p></div>;
    }
    if (stage === 2) return <div className="table-center"><span>{stageEyebrow[2]}</span><h2>Return to your{'\n'}own thinking.</h2><p>{improved} / {n} improved answers</p></div>;
    return <div className="table-center"><span>{stageEyebrow[3]}</span><h2>{world.finished ? 'One clearer\nanswer.' : world.synthP > 0 ? 'Bringing it\ntogether.' : 'Shuffling the\nimproved answers.'}</h2>{world.finished || (live && live.finalReady) ? <button className="primary small" onClick={onResult}>Open the final answer <ArrowRight size={14}/></button> : <p>{world.synthP > 0 ? 'Final writer at work' : 'Order randomised before synthesis'}</p>}</div>;
  };

  // ----- stage 4 papers
  const gathered = () => {
    if (stage !== 3) return null;
    const slotW = Math.min(46, (w * 0.5) / n); const y0 = cy + 104;
    return <>
      {seats.map((m, i) => {
        const from = chairPos(i);
        const slotIndex = world.shuffleP < 0.55 ? i : world.order.indexOf(i);
        const to = { x: cx + (slotIndex - (n - 1) / 2) * slotW, y: y0 };
        const p1 = easeOut(clamp01(world.shuffleP / 0.5));
        let x = from.x + (to.x - from.x) * p1, y = from.y + (to.y - from.y) * p1;
        let scale = 1, opacity = 1;
        if (world.synthP > 0) { const p2 = easeInOut(world.synthP); x += (cx - x) * p2; y += (cy - 6 - y) * p2; scale = 1 - 0.6 * p2; opacity = 1 - p2 * 0.9; }
        return <div key={m.name} className={'mini-paper ' + m.color} style={{ transform: `translate(${x}px, ${y}px) translate(-50%,-50%) scale(${scale})`, opacity }}><span>{m.letter}</span><i/><i/><i/></div>;
      })}
      {world.finalP > 0 && <div className="final-paper" style={{ transform: `translate(${cx}px, ${cy + 78}px) translate(-50%,-50%) scale(${0.6 + 0.4 * easeOut(world.finalP)})`, opacity: easeOut(world.finalP) }}><Sparkles size={14}/> Final answer</div>}
    </>;
  };

  // ----- matrix & list
  const matrix = () => <div className="matrix-view" style={{ gridTemplateColumns: `72px repeat(${n}, 1fr)` }}>
    <div className="mx-corner"><span>reviewer ↓</span><span>answer →</span></div>
    {seats.map(m => <div key={'h' + m.name} className="mx-head"><span className={'avatar small ' + m.color}>{m.letter}</span></div>)}
    {seats.map((r, ri) => <>
      <div key={'r' + r.name} className="mx-head"><span className={'avatar small ' + r.color}>{r.letter}</span><small>{r.name}</small></div>
      {seats.map((c, ci) => {
        if (ri === ci) return <div key={r.name + c.name} className="mx-cell own">own</div>;
        const done = world.tags[ci].some(tg => tg.reviewer === ri);
        const live = world.reviewing.some(x => x.reviewer === ri && x.chair === ci);
        return <div key={r.name + c.name} className={'mx-cell ' + (done ? 'done' : live ? 'live' : '')} style={done ? { background: seatHex[r.color] } : undefined}>{done ? <Check size={14}/> : live ? <Search size={13}/> : ''}</div>;
      })}
    </>)}
  </div>;
  const list = () => <div className="list-events">{script.events.filter(e => e.t <= t && e.ev.type !== 'run_started').slice().reverse().map((e, i) => { const [who, what] = describe(e); return <div key={i}><span className="mono">{fmt(e.t)}</span><p><strong>{who}</strong> {what}</p></div>; })}{!started && <p className="muted">Press play to see the event log.</p>}</div>;

  // ----- seat detail
  const detail = (i: number) => {
    const m = seats[i]; const p = stage === 0 ? world.think[i] : stage >= 2 ? world.improve[i] : 1;
    const words = seatText(i).split(' '); const shown = words.slice(0, Math.max(0, Math.round(words.length * p))).join(' ');
    const envelope = stage === 0 ? ['The question', 'Reference context, if any', m.name + '’s answerer prompt'] : stage === 1 ? ['The question', 'One anonymous answer', m.name + '’s reviewer prompt'] : stage === 2 ? ['The question', 'Its own first answer', (n - 1) + ' reviews written about it', m.name + '’s improver prompt'] : ['Not part of the synthesis call'];
    const tokens = Math.round(120 + 760 * p); const cost = (tokens / 1000) * 0.012;
    return <aside className="activity-panel glass seat-detail">
      <button className="text-button back" onClick={() => setSelected(null)}><ChevronLeft size={14}/> What’s happening</button>
      <div className="detail-head"><span className={'avatar ' + m.color}>{m.letter}</span><div><strong>{m.name}</strong><span>{m.provider} · seat {i + 1}</span></div></div>
      <p className="detail-status">{statusText(i)}</p>
      <div className="stream"><p>{shown}{p < 1 && started && <span className="caret"/>}</p></div>
      <p className="eyebrow">ALLOWED CONTEXT · THIS STAGE</p>
      <ul className="envelope-list">{envelope.map(x => <li key={x}><Mail size={12}/> {x}</li>)}</ul>
      <div className="detail-stats"><div><strong>{started ? tokens : 0}</strong><span>tokens</span></div><div><strong>{started ? (p * 6.4).toFixed(1) + 's' : '0.0s'}</strong><span>time</span></div><div><strong>${started ? cost.toFixed(3) : '0.000'}</strong><span>cost</span></div></div>
      <button className="secondary small" onClick={() => live ? live.onInspectSeat(i) : onInspect(seatIds[i])}>Open the full paper <ArrowUpRight size={13}/></button>
    </aside>;
  };

  // ----- svg overlay: rings, trails, flights
  const overlay = () => {
    const trails = hover !== null && stage >= 1 ? seats.map((m, j) => j === hover ? null : { j, a: chairPos(hover), b: chairPos(j), color: seatHex[m.color] }).filter(Boolean) as { j: number; a: { x: number; y: number }; b: { x: number; y: number }; color: string }[] : [];
    return <svg className="table-svg" width={w} height={h} aria-hidden="true">
      <ellipse cx={cx} cy={cy} rx={Rx} ry={R} className="ring dashed"/>
      <ellipse cx={cx} cy={cy} rx={tokenRx} ry={tokenR} className="ring"/>
      {trails.map(tr => { const c = curve(tr.a, tr.b, 0.5, 0.25); return <path key={tr.j} d={`M ${tr.a.x} ${tr.a.y} Q ${2 * c.x - (tr.a.x + tr.b.x) / 2} ${2 * c.y - (tr.a.y + tr.b.y) / 2} ${tr.b.x} ${tr.b.y}`} stroke={tr.color} className="trail"/>; })}
      {world.flights.map((f, i) => {
        const p = reducedMotion ? 1 : easeInOut(f.p);
        const from = f.kind === 'envelope' ? { x: cx, y: cy } : tokenPos(f.chair);
        const to = f.kind === 'envelope' ? tokenPos(f.chair) : chairPos(f.chair);
        const pt = curve(from, to, p); const color = seatHex[seats[f.reviewer].color];
        return f.kind === 'envelope'
          ? <g key={'f' + i} transform={`translate(${pt.x - 9}, ${pt.y - 6})`} className="envelope"><rect width="18" height="12" rx="2" fill="var(--surface)" stroke={color}/><path d="M0 0 L9 7 L18 0" fill="none" stroke={color}/></g>
          : <circle key={'f' + i} cx={pt.x} cy={pt.y} r="6" fill={color} className="tag-flight"/>;
      })}
    </svg>;
  };

  const seatCard = (m: SeatModel, i: number) => {
    const chairIndex = i; // chairs never move
    const pos = chairPos(chairIndex);
    const st = world.status[i];
    const paperKind = world.paper[i];
    const reviewingHere = world.reviewing.find(x => x.chair === chairIndex);
    const lines = paperKind === 'none' ? world.think[i] : 1;
    const dockP = paperKind === 'none' ? 0 : easeOut(world.paperP[i]);
    const tags = world.tags[chairIndex];
    const cls = ['table-seat', 'seat-' + m.color, 'n' + n, hover === i ? 'hover' : '', selected === i ? 'selected' : '', st === 'thinking' || st === 'improving' ? 'busy' : '', world.improve[i] > 0 && world.improve[i] < 1 ? 'pulse' : ''].join(' ');
    return <button key={m.name} className={cls} style={{ left: pos.x - cw / 2, top: pos.y - ch / 2, width: cw, height: ch }} onMouseEnter={() => setHover(i)} onMouseLeave={() => setHover(null)} onFocus={() => setHover(i)} onBlur={() => setHover(null)} onClick={() => setSelected(selected === i ? null : i)} aria-label={m.name + ' seat'}>
      <div className="seat-head"><strong>{m.name}</strong><small>{m.provider}</small>
        {started && (paperKind !== 'none' || st === 'reviewed' || st === 'fresh') && <span className="seat-state"><Check size={11}/></span>}
        {!started && n > 2 && <span className="seat-remove" role="button" aria-label={'Remove ' + m.name} onClick={e => { e.stopPropagation(); onRemoveSeat(seatIds[i]); }}><X size={11}/></span>}
      </div>
      {n <= 6 && <p className={'seat-status' + (st === 'clearing' ? ' clearing' : '')}>{statusText(i)}</p>}
      <div className="paper-slot">
        {(started && (paperKind !== 'none' || world.think[i] > 0)) && <div className={'paper ' + paperKind + (world.improve[i] > 0 && world.improve[i] < 1 ? ' folding' : '')} style={{ transform: `translateY(${(1 - dockP) * -8}px)`, opacity: paperKind === 'none' ? 0.7 : 1 }}>
          <span className="paper-tag">{paperKind === 'improved' ? 'v2' : paperKind === 'answer' ? 'v1' : '…'}</span>
          <i style={{ width: `${Math.min(92, lines * 92)}%` }}/><i style={{ width: `${Math.min(76, Math.max(0, lines - 0.3) * 110)}%` }}/><i style={{ width: `${Math.min(57, Math.max(0, lines - 0.6) * 140)}%` }}/>
          {reviewingHere && <span className="scan" style={{ top: `${18 + easeInOut(reviewingHere.p) * 60}%` }}/>}
          {paperKind === 'improved' && world.paperP[i] < 1 && <span className="shimmer"/>}
        </div>}
        {tags.length > 0 && <div className="tag-stack">{tags.map(tg => <span key={tg.reviewer} className="tag" style={{ background: seatHex[seats[tg.reviewer].color] }} title={'Reviewed by ' + seats[tg.reviewer].name}>{seats[tg.reviewer].letter}</span>)}</div>}
      </div>
    </button>;
  };

  const token = (m: SeatModel, i: number) => {
    const k = tokenK(i); const pos = tokenPos(k);
    const st = world.status[i];
    const glow = st === 'thinking' || st === 'reviewing' || st === 'improving' || st === 'opening';
    const away = world.rotation !== 0 || world.moveP < 1;
    const ring = st === 'thinking' ? world.think[i] : st === 'improving' ? world.improve[i] : 0;
    const c = 2 * Math.PI * 21;
    const mem = world.cleared[i] < 1 ? (world.cleared[i] < 0.5 ? 1 - world.cleared[i] * 2 : (world.cleared[i] - 0.5) * 2) : 1;
    return <div key={m.name} className={['token', m.color, glow ? 'glow' : '', away ? 'away' : '', world.moveP < 1 ? 'moving' : ''].join(' ')} style={{ transform: `translate(${pos.x}px, ${pos.y}px) translate(-50%,-50%)` }} title={m.name}>
      <svg viewBox="0 0 48 48" className="token-ring" aria-hidden="true"><circle cx="24" cy="24" r="21" className="track"/><circle cx="24" cy="24" r="21" className="fill" strokeDasharray={c} strokeDashoffset={c * (1 - ring)} style={{ opacity: ring > 0 && ring < 1 ? 1 : 0 }}/></svg>
      <span className={'avatar small ' + m.color}>{m.letter}</span>
      <span className="memory" aria-hidden="true"><i style={{ width: `${mem * 100}%`, background: world.cleared[i] < 1 ? '#e0a43a' : undefined }}/></span>
    </div>;
  };

  const progressFrac = stage === 0 ? answered / n : stage === 1 ? world.reviewsDone / world.reviewsTotal : stage === 2 ? improved / n : world.finished ? 1 : world.synthP;

  return <div className="page live-page">
    <div className="page-heading"><div><p className="eyebrow">THE ROUNDTABLE</p><h1>Different minds. Shared curiosity.</h1><p className="subtitle question-line">{question}</p></div>
      {live ? (live.active ? <button className="secondary" onClick={live.onCancel}>Cancel run</button> : <span className="live-status"><b>{live.state}</b>{live.reason ? ` · ${live.reason}` : ''}</span>) : <button className="secondary" onClick={() => { setPlaying(false); setCancelled(true); }}>Cancel demo</button>}</div>
    {live && (live.state === 'blocked' || live.state === 'interrupted') && <div className="notice">{live.reason ?? 'This run stopped.'} {live.uncertainCount > 0 ? <button onClick={live.onRetryUncertain}>Retry {live.uncertainCount} uncertain call{live.uncertainCount === 1 ? '' : 's'} (may be billed again)</button> : <button onClick={live.onResume}>Resume</button>}</div>}
    <div className="stage-strip" style={{ gridTemplateColumns: 'repeat(4, 1fr)' }}>{stageNames.map((s, i) => <button key={s} onClick={() => live ? openStage(i) : jump(i)} title={live ? 'Read the papers of this stage' : undefined} className={[(started && stage === i) ? 'current' : (started && stage > i) ? 'done' : '', live && view === 'stage' && stageView === i ? 'open' : ''].join(' ')}><span>{started && stage > i ? <Check size={15}/> : '0' + (i + 1)}</span><div><strong>{s}</strong><small>{[n + ' private responses', n * (n - 1) + ' isolated reviews', n + ' improved answers', 'One considered answer'][i]}{live ? ' · click to read' : ''}</small></div>{live?.webSearch[i] && <span className="stage-web" title="Web search was on for this stage"><Globe size={12}/> web</span>}</button>)}</div>
    {!live && cancelled && <div className="notice">Sample animation paused. Nothing was charged or sent. <button onClick={() => { setCancelled(false); setPlaying(true); }}>Resume demo</button></div>}
    <div className={"live-layout" + (view === 'stage' ? ' reading' : '')}>
      <section className="table-surface">
        <div className="surface-heading">
          <span><span className={'tiny-dot ' + (isPlaying ? 'green' : '')}/> {live ? (live.active ? 'Live run in progress' : `Run ${live.state}`) : playing ? 'Sample in progress' : started ? 'Sample paused' : 'Explore the sample'}</span>
          <div className="surface-tools">
            {!live && <button className="text-button" onClick={toggle} aria-label={playing ? 'Pause' : 'Play demo'}>{playing ? <Pause size={13}/> : <Play size={13}/>} {playing ? 'Pause sample' : t >= script.total ? 'Replay sample' : 'Play sample'}</button>}
            {!started && !live && <span className="seat-count"><button className="icon-button" aria-label="Remove a seat" disabled={n <= 2} onClick={() => onRemoveSeat(seatIds[n - 1])}><Minus size={14}/></button><b>{n} seats</b><button className="icon-button" aria-label="Add a seat" disabled={n >= maxSeats} onClick={() => { if (n >= maxSeats) notify('Eight seats is the maximum for one table.'); else onAddSeat(); }}><Plus size={14}/></button></span>}
            <div className="view-toggle" aria-label="View">
              <button aria-pressed={view === 'table'} className={view === 'table' ? 'on' : ''} onClick={() => setView('table')} aria-label="Show roundtable view"><LayoutGrid size={14}/> Table</button>
              <button aria-pressed={view === 'matrix'} className={view === 'matrix' ? 'on' : ''} onClick={() => setView('matrix')} aria-label="Show matrix view"><Search size={14}/> Matrix</button>
              <button aria-pressed={view === 'list'} className={view === 'list' ? 'on' : ''} onClick={() => setView('list')} aria-label="Show list view"><List size={14}/> List</button>
              {live && <button aria-pressed={view === 'stage'} className={view === 'stage' ? 'on' : ''} onClick={() => openStage(Math.min(stage, 3))} aria-label="Show papers"><FileText size={14}/> Papers</button>}
            </div>
          </div>
        </div>
        {view === 'table' && <div ref={tableRef} className={'roundtable n' + n + (reducedMotion ? ' reduced' : '')}>
          {overlay()}
          {centre()}
          {gathered()}
          {seats.map(seatCard)}
          {seats.map(token)}
        </div>}
        {view === 'matrix' && <div className="roundtable alt">{matrix()}</div>}
        {view === 'list' && <div className="roundtable alt list-view">{list()}</div>}
        {view === 'stage' && live && <StageDocs stage={stageView} seats={seats} papers={live.papers} planned={[n, n * (n - 1), n, 1][stageView]} onStage={openStage} onInspect={live.onInspectPaper} finalReady={live.finalReady} onOpenFinal={onResult}/>}
        {live && live.active && <div className="scrubber glass live-bar">
          <span className="tiny-dot green pulse-dot"/>
          <span className="live-bar-text"><b>Live</b> · {live.stageLabel} · {live.done} / {live.total} calls done · elapsed {live.elapsedLabel}</span>
          <span className="live-bar-note">Real requests in flight. Each stage box above opens its papers as they arrive.</span>
        </div>}
        {live && !live.active && <div className="scrubber glass live-bar">
          <span className={'tiny-dot ' + (live.finalReady ? 'green' : '')}/>
          <span className="live-bar-text"><b>{live.finalReady ? 'Completed' : live.state}</b> · {live.done} / {live.total} calls · spent ${live.settledUsd}</span>
          <span className="live-bar-note">Click a stage box above to read every paper of that stage.</span>
        </div>}
        <footer className="table-footer"><span><ShieldCheck size={15}/> Each review sees one answer. Nothing else.</span><span><LockKeyhole size={13}/> {live ? 'Every paper here is a committed record; nothing is simulated.' : 'Sample data · no live AI requests'}</span></footer>
      </section>
      {view === 'stage' ? null : selected !== null && selected < n ? detail(selected) : <aside className="activity-panel glass">
        <p className="eyebrow">WHAT’S HAPPENING</p>
        <h2>{!started ? 'Ready to begin.' : ['Ideas take shape.', 'Better, together.', 'Time to reflect.', 'The final perspective.'][stage]}</h2>
        <p className="muted">{!started ? 'Every seat answers alone first. Then the papers stay and the minds move.' : stageCopy[stage]}</p>
        <div className="review-progress"><strong>{stage === 0 ? answered : stage === 1 ? world.reviewsDone : stage === 2 ? improved : world.finished ? 1 : 0}<span> / {stage === 1 ? world.reviewsTotal : stage === 3 ? 1 : n}</span></strong><p>{['answers ready', 'reviews complete', 'improved answers', 'final answer'][stage]}</p><div><i style={{ width: `${progressFrac * 100}%` }}/></div></div>
        <div className="activity-events">{eventsSoFar.map((e, i) => { const [who, what] = describe(e); return <div key={e.t + '-' + i} className="event-in"><span className="event-check"><Check size={12}/></span><p><strong>{who}</strong> {what}<small>{fmt(e.t)} · {live ? 'committed' : 'sample event'}</small></p></div>; })}{!started && <p className="muted small">Hover a seat to see who reviews it. Click a seat to follow it.</p>}</div>
        <div className="isolation-note"><ShieldCheck size={20}/><strong>Perspective without carryover.</strong><p>Review history is never passed into the next review. Each call gets only the inputs it needs.</p></div>
      </aside>}
    </div>
    <div className="session-bottom">{live ? <span className="live-status"><span className="sample-label">LIVE</span> Spent <b>${live.settledUsd}</b>{live.unresolvedUsd !== '0.00' && <> · unresolved <b>${live.unresolvedUsd}</b></>}</span> : <span><span className="sample-label">UI SAMPLE</span> Simulated activity · no live AI requests</span>}<button className="primary" disabled={!!live && !live.finalReady} onClick={() => { setPlaying(false); onResult(); }}>{live ? 'Open the final answer' : 'View sample answer'} <ArrowRight size={16}/></button></div>
  </div>;
}
