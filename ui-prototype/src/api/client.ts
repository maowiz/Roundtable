// Typed IPC client. In the desktop app every call goes through Tauri's invoke; in the
// browser preview `isDesktop()` is false and screens keep using the bundled sample.

import type { EventCallback, UnlistenFn } from '@tauri-apps/api/event';

export type RunState = 'draft' | 'validating' | 'answering' | 'reviewing' | 'improving' | 'synthesizing' | 'completed' | 'blocked' | 'interrupted' | 'cancelling' | 'cancelled' | 'failed';
export type Stage = 'answer' | 'review' | 'improve' | 'synthesize';
export type CallState = 'planned' | 'ready' | 'in_flight' | 'retry_wait' | 'accepted' | 'uncertain' | 'refused' | 'invalid' | 'failed';

export interface CredentialMeta { ref: string; provider: string; label: string; createdAt: string; lastTestedAt: string | null; lastTestResult: string | null; present: boolean; fingerprint: string | null }
export interface TestResult { ok: boolean; category: string | null; message: string; returnedModel: string | null; latencyMs: number }
export interface RoleTuning { temperature: number | null; maxOutputWords: number; reasoningEffort: string | null }
export interface SeatDoc { label: string; provider: string; credentialRef: string; modelId: string; modelIsAlias?: boolean; answer: RoleTuning; review: RoleTuning; improve: RoleTuning; promptAnswer: string; promptReview: string; promptImprove: string; costCapUsd: string }
export interface SynthesizerDoc { provider: string; credentialRef: string; modelId: string; modelIsAlias?: boolean; tuning: RoleTuning; prompt: string }
export interface WebSearchDoc { answer: boolean; review: boolean; improve: boolean; synthesize: boolean }
export const WEB_SEARCH_OFF: WebSearchDoc = { answer: false, review: false, improve: false, synthesize: false };
export interface PanelDoc { id: string | null; name: string; seats: SeatDoc[]; synthesizer: SynthesizerDoc; webSearch?: WebSearchDoc }
export interface PanelSummary { id: string; name: string; seatCount: number; doc: PanelDoc }
export interface ModelInfo { provider: string; modelId: string; priceVerified: boolean; supportsTemperature: boolean; supportsReasoningEffort: boolean }
export interface PreflightRequest { question: string; referenceText: string | null; panelId: string; ceilingUsd: string }
export interface PreflightReport { ok: boolean; seatCount: number; baseCalls: number; maxAttempts: number; estimateLowUsd: string; estimateHighUsd: string; unverifiedPrices: string[]; problems: string[]; warnings: string[]; webSearchCalls: number; webSearchFeeUsd: string }
export interface RunSummary { id: string; question: string; state: RunState; stage: Stage; reasonCode: string | null; reasonText: string | null; createdAt: string; finishedAt: string | null; seatCount: number; isSample: boolean; settledUsd: string; unresolvedUsd: string }
export interface SeatSummary { index: number; label: string; provider: string; modelId: string; modelReturnedId: string | null }
export interface CallSummary { id: string; stage: Stage; reviewerSeat: number; targetSeat: number | null; rotation: number | null; state: CallState; attemptsUsed: number; acceptedPaperId: string | null }
export interface PaperMeta { id: string; kind: string; authorSeat: number; targetSeat: number | null }
export interface Coverage { stage: Stage; planned: number; accepted: number }
export interface RunSnapshot { run: RunSummary; seats: SeatSummary[]; synthesizer: { provider: string; modelId: string }; calls: CallSummary[]; papers: PaperMeta[]; coverage: Coverage[]; lastSeq: number; finalPaper: Record<string, unknown> | null; webSearch: WebSearchDoc; usage: { inputTokens: number; outputTokens: number; reasoningTokens: number; attempts: number }; inFlight: { seat: number; stage: Stage; attempt: number; since: string }[] }
export interface EventOut { seq: number; kind: string; payload: Record<string, unknown> }
export interface RunEvent extends EventOut { run_id: string }
export interface PaperDetail { paper: PaperMeta; body: Record<string, unknown>; digest: string; views: { id: string; transformVersion: string; text: string; digest: string; possiblyIdentifiable: boolean }[]; envelope: { stage: string; instructions: string; messages: { role: string; content: string }[]; schemaId: string; aliasMap: unknown; digest: string } | null; attempts: Record<string, unknown>[] }
export interface SystemPrompts { common: string; answer: string; review: string; improve: string; synthesize: string }
export interface SystemPromptsState { current: SystemPrompts; defaults: SystemPrompts; isDefault: boolean; version: string }
export interface PromptPreview { provider: string; model: string; url: string; system: string; user: string; request: unknown }
export interface AppInfo { appVersion: string; priceCatalog: string; providerCatalog: string; dataDir: string; schemaVersion: number }

declare global {
  interface Window { __TAURI_INTERNALS__?: unknown }
}

export function isDesktop(): boolean {
  return typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isDesktop()) throw new Error('Desktop features are not available in the browser preview.');
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

export const api = {
  appInfo: () => call<AppInfo>('app_info'),
  listCredentials: () => call<CredentialMeta[]>('list_credentials'),
  saveCredential: (provider: string, label: string, secret: string) => call<string>('save_credential', { provider, label, secret }),
  testCredential: (ref: string, provider: string, modelId: string) => call<TestResult>('test_credential', { ref, provider, modelId }),
  deleteCredential: (ref: string) => call<void>('delete_credential', { ref }),
  listProviders: () => call<string[]>('list_providers'),
  listModels: (provider: string) => call<ModelInfo[]>('list_models', { provider }),
  listPanels: () => call<PanelSummary[]>('list_panels'),
  savePanel: (doc: PanelDoc) => call<string>('save_panel', { doc }),
  deletePanel: (id: string) => call<boolean>('delete_panel', { id }),
  preflightRun: (req: PreflightRequest) => call<PreflightReport>('preflight_run', { req }),
  startRun: (req: PreflightRequest) => call<string>('start_run', { req }),
  cancelRun: (runId: string) => call<void>('cancel_run', { runId }),
  resumeRun: (runId: string) => call<void>('resume_run', { runId }),
  retryUncertain: (callId: string) => call<void>('retry_uncertain', { callId }),
  getRunSnapshot: (runId: string) => call<RunSnapshot>('get_run_snapshot', { runId }),
  getEvents: (runId: string, afterSeq: number) => call<EventOut[]>('get_events', { runId, afterSeq }),
  listRunPapers: (runId: string) => call<{ id: string; kind: string; authorSeat: number; targetSeat: number | null; body: Record<string, unknown>; sources: { url: string; title: string }[] }[]>('list_run_papers', { runId }),
  inspectPaper: (paperId: string) => call<PaperDetail>('inspect_paper', { paperId }),
  listRuns: (query?: string, limit?: number) => call<RunSummary[]>('list_runs', { query: query ?? null, limit: limit ?? null }),
  deleteRun: (runId: string) => call<boolean>('delete_run', { runId }),
  getSettings: () => call<Record<string, unknown>>('get_settings'),
  saveSettings: (settings: Record<string, unknown>) => call<void>('save_settings', { settings }),
  getSystemPrompts: () => call<SystemPromptsState>('get_system_prompts'),
  saveSystemPrompts: (prompts: SystemPrompts | null) => call<void>('save_system_prompts', { prompts }),
  previewPrompt: (req: { doc: PanelDoc; seat: number | null; stage: 'answer' | 'review' | 'improve' | 'synthesize'; system: SystemPrompts; question: string | null }) => call<PromptPreview>('preview_prompt', { req }),
};

/// Subscribe to committed engine events. Returns an unlisten function.
export async function onRunEvent(handler: (ev: RunEvent) => void): Promise<UnlistenFn> {
  if (!isDesktop()) return () => {};
  const { listen } = await import('@tauri-apps/api/event');
  const cb: EventCallback<RunEvent> = e => handler(e.payload);
  return listen<RunEvent>('run-event', cb);
}

/// Attach a run: load the snapshot, replay any events after its sequence, then stream.
/// Gaps (lagged subscriber) are healed by re-reading from the ledger.
export async function attachRun(runId: string, onEvent: (ev: EventOut) => void, onSnapshot: (s: RunSnapshot) => void): Promise<UnlistenFn> {
  const snap = await api.getRunSnapshot(runId);
  onSnapshot(snap);
  let last = snap.lastSeq;
  let healing = false;
  const heal = async () => {
    if (healing) return;
    healing = true;
    try {
      const missed = await api.getEvents(runId, last);
      for (const ev of missed) { if (ev.seq > last) { last = ev.seq; onEvent(ev); } }
    } finally { healing = false; }
  };
  const un = await onRunEvent(ev => {
    if (ev.run_id !== runId) return;
    if (ev.seq <= last) return;
    if (ev.seq !== last + 1) { void heal(); return; }
    last = ev.seq;
    onEvent(ev);
  });
  // events committed between the snapshot and the subscription
  await heal();
  return un;
}
