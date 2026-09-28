// Maps committed engine events onto the table's event vocabulary so the same renderer
// plays a real run at true timing (plan D-B3). Times are milliseconds since the first event.

import type { EventOut, RunSnapshot } from '../api/client';
import type { Ev, Script, TimedEv } from './script';

const STAGE_INDEX: Record<string, number> = { answer: 0, review: 1, improve: 2, synthesize: 3 };

export interface LiveFeed {
  script: Script;
  /** Timeline position for "now" (ms since start). */
  now: number;
  finished: boolean;
  runState: string;
  reason: string | null;
  /** Per call id: the timeline index of its open thinking/review entry, so it can be closed on accept. */
  open: Map<string, number>;
  startedAt: number;
  n: number;
  reviewsDone: number;
}

export function newFeed(snapshot: RunSnapshot): LiveFeed {
  const n = snapshot.seats.length;
  const script: Script = { events: [], stageAt: [0, 0, 0, 0], total: 1, n, cinematic: false };
  const feed: LiveFeed = { script, now: 0, finished: false, runState: snapshot.run.state, reason: snapshot.run.reasonText, open: new Map(), startedAt: Date.now(), n, reviewsDone: 0 };
  // Rebuild from the persisted events so history/replay and mid-run attaches look identical.
  return feed;
}

function push(feed: LiveFeed, t: number, d: number, ev: Ev): number {
  feed.script.events.push({ t, d, ev });
  feed.script.total = Math.max(feed.script.total, t + d + 200);
  return feed.script.events.length - 1;
}

const LONG = 10 * 60 * 1000; // "open" duration until the matching accept arrives

/** Apply one engine event at timeline position `t`. */
export function applyEvent(feed: LiveFeed, ev: EventOut, t: number): void {
  const p = ev.payload as Record<string, any>;
  const s = feed.script;
  feed.now = Math.max(feed.now, t);
  switch (ev.kind) {
    case 'run.state_changed': {
      feed.runState = String(p.state ?? feed.runState);
      feed.reason = (p.reason_text as string) ?? feed.reason;
      if (p.state === 'answering' && s.events.length === 0) push(feed, t, 600, { type: 'run_started' });
      const idx = STAGE_INDEX[String(p.stage)];
      if (idx !== undefined && s.stageAt[idx] === 0 && idx > 0) s.stageAt[idx] = t;
      break;
    }
    case 'stage.started': {
      const idx = STAGE_INDEX[String(p.stage)];
      if (idx !== undefined && (idx === 0 || s.stageAt[idx] === 0)) s.stageAt[idx] = t;
      if (p.stage === 'review') push(feed, t, 700, { type: 'rotation_started', index: 1 });
      if (p.stage === 'improve') push(feed, t, 700, { type: 'seats_returned' });
      if (p.stage === 'synthesize') push(feed, t, 900, { type: 'shuffle', order: Array.from({ length: feed.n }, (_, i) => i) });
      break;
    }
    case 'call.started': {
      const stage = String(p.stage);
      const seat = Number(p.reviewer);
      const id = String(p.call_id);
      // a retry restarts the same call: close the earlier open entry so it does not stay "thinking" forever
      const prev = feed.open.get(id);
      if (prev !== undefined) { const e = s.events[prev]; e.d = Math.max(50, t - e.t); feed.open.delete(id); }
      if (stage === 'answer') feed.open.set(id, push(feed, t, LONG, { type: 'seat_thinking', seat }));
      else if (stage === 'review') {
        const chair = Number(p.target);
        push(feed, t, 400, { type: 'envelope_sent', reviewer: seat, chair });
        feed.open.set(id, push(feed, t + 400, LONG, { type: 'review_started', reviewer: seat, chair }));
      } else if (stage === 'improve') feed.open.set(id, push(feed, t, LONG, { type: 'improve_started', seat }));
      else if (stage === 'synthesize') feed.open.set(id, push(feed, t, LONG, { type: 'synthesis_started' }));
      break;
    }
    case 'call.accepted': {
      const stage = String(p.stage);
      const seat = Number(p.reviewer);
      const id = String(p.call_id);
      const openIdx = feed.open.get(id);
      if (openIdx !== undefined) { const e = s.events[openIdx]; e.d = Math.max(50, t - e.t); feed.open.delete(id); }
      if (stage === 'answer') push(feed, t, 400, { type: 'paper_created', seat, kind: 'answer' });
      else if (stage === 'review') { push(feed, t, 400, { type: 'review_done', reviewer: seat, chair: Number(p.target) }); feed.reviewsDone++; }
      else if (stage === 'improve') push(feed, t, 400, { type: 'paper_created', seat, kind: 'improved' });
      else if (stage === 'synthesize') push(feed, t, 500, { type: 'paper_created', seat: -1, kind: 'final' });
      break;
    }
    case 'call.retry_wait':
    case 'call.invalid':
    case 'call.failed':
    case 'call.refused':
    case 'call.uncertain': {
      const id = String(p.call_id);
      const openIdx = feed.open.get(id);
      if (openIdx !== undefined) { const e = s.events[openIdx]; e.d = Math.max(50, t - e.t); feed.open.delete(id); }
      break;
    }
    case 'stage.completed': {
      if (p.stage === 'review') for (let i = 0; i < feed.n; i++) push(feed, t + i * 30, 450, { type: 'context_cleared', seat: i });
      break;
    }
    case 'run.completed': {
      push(feed, t, 300, { type: 'run_finished' });
      feed.finished = true;
      break;
    }
    default:
      break;
  }
  // keep the playhead at the front of the timeline
  s.total = Math.max(s.total, feed.now + 400);
}

/** Feed persisted events (with their ledger timestamps) so replays keep true timing. */
export function replayPersisted(feed: LiveFeed, events: { seq: number; kind: string; payload: Record<string, unknown>; at?: number }[]): void {
  let t0: number | null = null;
  for (const ev of events) {
    const at = ev.at ?? 0;
    if (t0 === null) t0 = at;
    applyEvent(feed, ev, at - t0);
  }
}

export type { TimedEv };
