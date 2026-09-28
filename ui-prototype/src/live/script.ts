// Event timeline for the Live Roundtable.
// The demo plays a generated script; the real engine will emit the same event names.

export type Ev =
  | { type: 'run_started' }
  | { type: 'seat_thinking'; seat: number }
  | { type: 'paper_created'; seat: number; kind: 'answer' | 'improved' | 'final' }
  | { type: 'rotation_started'; index: number }
  | { type: 'envelope_sent'; reviewer: number; chair: number }
  | { type: 'review_started'; reviewer: number; chair: number }
  | { type: 'review_done'; reviewer: number; chair: number }
  | { type: 'context_cleared'; seat: number }
  | { type: 'seats_returned' }
  | { type: 'improve_started'; seat: number }
  | { type: 'shuffle'; order: number[] }
  | { type: 'synthesis_started' }
  | { type: 'run_finished' };

export interface TimedEv { t: number; d: number; ev: Ev }
export interface Script { events: TimedEv[]; stageAt: number[]; total: number; n: number; cinematic: boolean }

export function buildScript(n: number, cinematic = true): Script {
  const ev: TimedEv[] = [];
  const push = (t: number, d: number, e: Ev) => ev.push({ t, d, ev: e });
  let t = 0;
  push(0, 900, { type: 'run_started' });

  // Stage 1: independent answers, all at once
  const s1 = 900;
  for (let i = 0; i < n; i++) push(s1 + i * 140, 2400, { type: 'seat_thinking', seat: i });
  for (let i = 0; i < n; i++) push(s1 + 2500 + i * 180, 520, { type: 'paper_created', seat: i, kind: 'answer' });
  t = s1 + 2500 + n * 180 + 600;

  // Stage 2: rotate and review
  const s2 = t;
  for (let r = 1; r < n; r++) {
    push(t, 900, { type: 'rotation_started', index: r });
    t += 900;
    if (cinematic) {
      for (let i = 0; i < n; i++) {
        const chair = (i + r) % n;
        const b = t + i * 900;
        push(b, 420, { type: 'envelope_sent', reviewer: i, chair });
        push(b + 420, 380, { type: 'review_started', reviewer: i, chair });
        push(b + 800, 420, { type: 'review_done', reviewer: i, chair });
      }
      t += n * 900 + 200;
    } else {
      for (let i = 0; i < n; i++) {
        const chair = (i + r) % n;
        push(t + i * 60, 420, { type: 'envelope_sent', reviewer: i, chair });
        push(t + 480 + i * 60, 900 + (i % 3) * 250, { type: 'review_started', reviewer: i, chair });
        push(t + 1400 + i * 130 + (i % 3) * 250, 420, { type: 'review_done', reviewer: i, chair });
      }
      t += 1400 + n * 130 + 700 + 500;
    }
    for (let i = 0; i < n; i++) push(t + i * 40, 480, { type: 'context_cleared', seat: i });
    t += 700;
  }

  // Stage 3: return home and improve
  const s3 = t;
  push(t, 950, { type: 'seats_returned' });
  t += 950;
  for (let i = 0; i < n; i++) push(t + i * 120, 2100, { type: 'improve_started', seat: i });
  for (let i = 0; i < n; i++) push(t + 2200 + i * 170, 520, { type: 'paper_created', seat: i, kind: 'improved' });
  t += 2200 + n * 170 + 600;

  // Stage 4: shuffle and synthesise
  const s4 = t;
  const order = Array.from({ length: n }, (_, i) => i);
  // deterministic shuffle so replays match
  for (let i = n - 1; i > 0; i--) { const j = (i * 7 + 3) % (i + 1); [order[i], order[j]] = [order[j], order[i]]; }
  push(t, 1500, { type: 'shuffle', order });
  t += 1500;
  push(t, 2300, { type: 'synthesis_started' });
  t += 2300;
  push(t, 700, { type: 'paper_created', seat: -1, kind: 'final' });
  t += 700;
  push(t, 400, { type: 'run_finished' });
  t += 400;

  return { events: ev, stageAt: [s1, s2, s3, s4], total: t, n, cinematic };
}

export const clamp01 = (x: number) => (x < 0 ? 0 : x > 1 ? 1 : x);
export const easeInOut = (p: number) => (p < 0.5 ? 2 * p * p : 1 - Math.pow(-2 * p + 2, 2) / 2);
export const easeOut = (p: number) => 1 - Math.pow(1 - p, 3);

export interface Flight { kind: 'envelope' | 'tag'; reviewer: number; chair: number; p: number }
export interface World {
  started: boolean;
  stage: number;               // 0..3
  rotation: number;            // chairs moved so far (0 = home)
  moveFrom: number; moveP: number; // interpolation for avatar moves (moveP 1 = settled)
  status: string[];            // per seat
  think: number[];             // 0..1 answer progress
  improve: number[];           // 0..1 improve progress
  paper: ('none' | 'answer' | 'improved')[];
  paperP: number[];            // docking animation progress
  tags: { reviewer: number; p: number }[][]; // per chair
  reviewing: { reviewer: number; chair: number; p: number }[];
  cleared: number[];           // context wipe progress per seat (1 = idle)
  flights: Flight[];
  reviewsDone: number; reviewsTotal: number;
  shuffleP: number; order: number[]; synthP: number; finalP: number; finished: boolean;
  rotationIndex: number;       // current rotation number for the label
}

export function derive(s: Script, t: number): World {
  const n = s.n;
  const w: World = {
    started: t > 0, stage: 0, rotation: 0, moveFrom: 0, moveP: 1,
    status: Array(n).fill('waiting'), think: Array(n).fill(0), improve: Array(n).fill(0),
    paper: Array(n).fill('none'), paperP: Array(n).fill(0), tags: Array.from({ length: n }, () => []),
    reviewing: [], cleared: Array(n).fill(1), flights: [], reviewsDone: 0, reviewsTotal: n * (n - 1),
    shuffleP: 0, order: Array.from({ length: n }, (_, i) => i), synthP: 0, finalP: 0, finished: false, rotationIndex: 0,
  };
  w.stage = t >= s.stageAt[3] ? 3 : t >= s.stageAt[2] ? 2 : t >= s.stageAt[1] ? 1 : 0;
  for (const { t: t0, d, ev } of s.events) {
    if (t < t0) continue;
    const p = clamp01((t - t0) / d);
    switch (ev.type) {
      case 'seat_thinking': w.status[ev.seat] = p < 1 ? 'thinking' : 'answered'; w.think[ev.seat] = p; break;
      case 'paper_created':
        if (ev.kind === 'final') { w.finalP = p; break; }
        w.paper[ev.seat] = ev.kind; w.paperP[ev.seat] = p; if (ev.kind === 'improved') w.status[ev.seat] = 'improved'; break;
      case 'rotation_started':
        w.rotationIndex = ev.index; w.rotation = ev.index; w.moveFrom = ev.index - 1; w.moveP = p;
        for (let i = 0; i < n; i++) w.status[i] = p < 1 ? 'moving' : 'seated';
        break;
      case 'envelope_sent':
        if (p < 1) { w.flights.push({ kind: 'envelope', reviewer: ev.reviewer, chair: ev.chair, p }); w.status[ev.reviewer] = 'opening'; }
        break;
      case 'review_started':
        w.status[ev.reviewer] = 'reviewing';
        if (p < 1) w.reviewing.push({ reviewer: ev.reviewer, chair: ev.chair, p });
        break;
      case 'review_done':
        w.status[ev.reviewer] = 'reviewed';
        if (p < 1) w.flights.push({ kind: 'tag', reviewer: ev.reviewer, chair: ev.chair, p });
        else { w.tags[ev.chair].push({ reviewer: ev.reviewer, p: 1 }); w.reviewsDone++; }
        break;
      case 'context_cleared': w.cleared[ev.seat] = p; w.status[ev.seat] = p < 1 ? 'clearing' : 'fresh'; break;
      case 'seats_returned': w.moveFrom = n - 1; w.rotation = n; w.moveP = p; for (let i = 0; i < n; i++) w.status[i] = p < 1 ? 'returning' : 'home'; break;
      case 'improve_started': w.status[ev.seat] = p < 1 ? 'improving' : 'improved'; w.improve[ev.seat] = p; break;
      case 'shuffle': w.shuffleP = p; w.order = ev.order; break;
      case 'synthesis_started': w.synthP = p; break;
      case 'run_finished': w.finished = true; break;
      default: break;
    }
  }
  if (w.rotation >= n) w.rotation = 0; // home again after the full circle
  return w;
}

export function nextEventTime(s: Script, t: number, dir: 1 | -1): number {
  const times = Array.from(new Set(s.events.map(e => e.t))).sort((a, b) => a - b);
  if (dir > 0) return times.find(x => x > t + 1) ?? s.total;
  const prev = times.filter(x => x < t - 1);
  return prev.length ? prev[prev.length - 1] : 0;
}
