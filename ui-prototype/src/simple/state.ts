// Data hooks for the simplified desktop app: the one agent team, the models the user can pick
// (only from providers whose key is saved), the shared system prompts, and small app settings.
// Every edit saves by itself; there is no Save button anywhere.

import { useCallback, useEffect, useRef, useState } from 'react';
import { api, WEB_SEARCH_OFF, type CredentialMeta, type PanelDoc, type SeatDoc, type SystemPrompts } from '../api/client';
import { DEFAULT_PROMPTS, providerName, seatLabel } from '../desktop';

export interface ModelOption {
  key: string;
  provider: string;
  providerLabel: string;
  credentialRef: string;
  modelId: string;
  verified: boolean;
  supportsTemperature: boolean;
}

export const MIN_AGENTS = 2;
export const MAX_AGENTS = 8;

function debounce(fn: () => void, ms: number, timer: { current: number | undefined }) {
  window.clearTimeout(timer.current);
  timer.current = window.setTimeout(fn, ms);
}

/** Models from every connected provider, one entry per model. */
export async function loadModelOptions(creds: CredentialMeta[]): Promise<ModelOption[]> {
  const byProvider = new Map<string, CredentialMeta>();
  for (const c of creds) if (c.present && !byProvider.has(c.provider)) byProvider.set(c.provider, c);
  const out: ModelOption[] = [];
  for (const [provider, cred] of byProvider) {
    const models = await api.listModels(provider).catch(() => []);
    for (const m of models) out.push({ key: provider + '|' + m.modelId, provider, providerLabel: providerName(provider), credentialRef: cred.ref, modelId: m.modelId, verified: m.priceVerified, supportsTemperature: m.supportsTemperature });
  }
  return out;
}

export function uniqueName(base: string, taken: string[]): string {
  if (!taken.includes(base)) return base;
  for (let i = 2; ; i++) if (!taken.includes(`${base} ${i}`)) return `${base} ${i}`;
}

export type Step = 'answer' | 'review' | 'improve' | 'synthesize';
const PARAGRAPH = String.fromCharCode(10, 10);
const ROLE_DEFAULT: Record<Step, string> = { answer: DEFAULT_PROMPTS.answer, review: DEFAULT_PROMPTS.review, improve: DEFAULT_PROMPTS.improve, synthesize: DEFAULT_PROMPTS.synth };

/** One agent's complete system prompt for one step: the app's rules for that step plus the agent's own instructions. */
export function fullPrompt(rules: SystemPrompts, step: Step, own: string): string {
  return [rules.common, rules[step], own].map(t => t.trim()).filter(Boolean).join(PARAGRAPH);
}

/** Everything a brand-new agent is told at each step. */
export function defaultPrompt(defaults: SystemPrompts | null, step: Step): string {
  return defaults ? fullPrompt(defaults, step, ROLE_DEFAULT[step]) : ROLE_DEFAULT[step];
}

export function newAgent(opt: ModelOption, taken: string[], words: number, defaults: SystemPrompts | null): SeatDoc {
  const t = (temp: number) => ({ temperature: opt.supportsTemperature ? temp : null, maxOutputWords: words, reasoningEffort: null });
  return { label: uniqueName(seatLabel(opt.modelId), taken), provider: opt.provider, credentialRef: opt.credentialRef, modelId: opt.modelId, answer: t(0.7), review: t(0.2), improve: t(0.4), promptAnswer: defaultPrompt(defaults, 'answer'), promptReview: defaultPrompt(defaults, 'review'), promptImprove: defaultPrompt(defaults, 'improve'), costCapUsd: '0.50' };
}

const EMPTY_RULES: SystemPrompts = { common: '', answer: '', review: '', improve: '', synthesize: '' };
const isEmptyRules = (p: SystemPrompts) => Object.values(p).every(v => !v.trim());

/** Fold the app-wide rules into each agent's own prompts (once), so every agent holds its whole prompt. */
function foldRules(doc: PanelDoc, rules: SystemPrompts): PanelDoc {
  const fold = (step: Step, own: string) => (rules[step].trim() && own.includes(rules[step].trim()) ? own : fullPrompt(rules, step, own));
  return {
    ...doc,
    seats: doc.seats.map(s => ({ ...s, promptAnswer: fold('answer', s.promptAnswer), promptReview: fold('review', s.promptReview), promptImprove: fold('improve', s.promptImprove) })),
    synthesizer: { ...doc.synthesizer, prompt: fold('synthesize', doc.synthesizer.prompt) },
  };
}

const DEFAULT_SEAT_MODELS = ['google/gemini-3.8-flash', 'z-ai/glm-5.3-flash', 'deepseek/deepseek-v4-pro-0813', 'qwen/qwen3.8-max-0902'];

/** Point a seat at another model, keeping its name and prompts; temperatures follow what the model accepts. */
export function withModel(seat: SeatDoc, opt: ModelOption): SeatDoc {
  const fix = (t: SeatDoc['answer'], d: number) => ({ ...t, temperature: opt.supportsTemperature ? (t.temperature ?? d) : null });
  return { ...seat, provider: opt.provider, credentialRef: opt.credentialRef, modelId: opt.modelId, answer: fix(seat.answer, 0.7), review: fix(seat.review, 0.2), improve: fix(seat.improve, 0.4) };
}

function teamFromOptions(options: ModelOption[], defaults: SystemPrompts | null): PanelDoc | null {
  const usable = options.filter(o => o.verified);
  if (usable.length === 0) return null;
  const preferred = DEFAULT_SEAT_MODELS.map(m => usable.find(o => o.modelId === m)).filter((o): o is ModelOption => !!o);
  const picks = preferred.length >= 2 ? preferred : [usable[0], usable[1 % usable.length]];
  const seats: SeatDoc[] = [];
  for (const o of picks) seats.push(newAgent(o, seats.map(s => s.label), 700, defaults));
  const w = picks[0];
  return { id: null, name: 'Roundtable team', seats, synthesizer: { provider: w.provider, credentialRef: w.credentialRef, modelId: w.modelId, tuning: { temperature: null, maxOutputWords: 1500, reasoningEffort: null }, prompt: defaultPrompt(defaults, 'synthesize') }, webSearch: { ...WEB_SEARCH_OFF } };
}

export type TeamStatus = 'loading' | 'ready' | 'needs-key' | 'no-models';

/** The single agent team the app works with. Edits apply at once and save after a short pause. */
export function useTeam(notify: (m: string) => void) {
  const [team, setTeam] = useState<PanelDoc | null>(null);
  const [options, setOptions] = useState<ModelOption[]>([]);
  const [status, setStatus] = useState<TeamStatus>('loading');
  const [savedVersion, setSavedVersion] = useState(0);
  const [defaults, setDefaults] = useState<SystemPrompts | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const pending = useRef<PanelDoc | null>(null);
  const saving = useRef<Promise<void> | null>(null);

  const reload = useCallback(async () => {
    try {
      const rules = await api.getSystemPrompts();
      setDefaults(rules.defaults);
      const creds = await api.listCredentials();
      const opts = await loadModelOptions(creds);
      setOptions(opts);
      if (!creds.some(c => c.present)) { setTeam(null); setStatus('needs-key'); return; }
      let panels = await api.listPanels();
      if (panels.length === 0) {
        const doc = teamFromOptions(opts, rules.defaults);
        if (!doc) { setTeam(null); setStatus('no-models'); return; }
        await api.savePanel(doc);
        panels = await api.listPanels();
      }
      // One-time move: the app-wide rules become part of every agent's own prompt, then stop being added.
      if (!isEmptyRules(rules.current)) {
        for (const p of panels) await api.savePanel(foldRules({ ...p.doc, id: p.id }, rules.current));
        await api.saveSystemPrompts(EMPTY_RULES);
        panels = await api.listPanels();
      }
      const doc = panels[0].doc;
      setTeam({ ...doc, id: panels[0].id, webSearch: doc.webSearch ?? { ...WEB_SEARCH_OFF } });
      setStatus('ready');
      setSavedVersion(v => v + 1);
    } catch (e) { notify(String(e)); }
  }, [notify]);

  useEffect(() => { void reload(); }, [reload]);

  const write = useCallback(async () => {
    const doc = pending.current;
    if (!doc) return;
    pending.current = null;
    try {
      const id = await api.savePanel(doc);
      if (!doc.id) setTeam(t => (t ? { ...t, id } : t));
      setSavedVersion(v => v + 1);
    } catch (e) { notify(String(e)); }
  }, [notify]);

  const update = useCallback((next: PanelDoc) => {
    setTeam(next);
    pending.current = next;
    debounce(() => { saving.current = write(); }, 450, timer);
  }, [write]);

  /** Save anything still waiting, before a run starts or a preview is built. */
  const flush = useCallback(async () => {
    window.clearTimeout(timer.current);
    if (pending.current) await write();
    else if (saving.current) await saving.current;
  }, [write]);

  return { team, options, status, savedVersion, update, flush, reload, defaults };
}

export type TeamApi = ReturnType<typeof useTeam>;

export interface AppSettings { theme: 'dark' | 'light'; ceilingUsd: string }
const DEFAULT_SETTINGS: AppSettings = { theme: 'dark', ceilingUsd: '1.00' };

/** Theme and spending limit, stored in the ledger's settings row. */
export function useAppSettings() {
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const raw = useRef<Record<string, unknown>>({});
  useEffect(() => {
    api.getSettings().then(s => {
      raw.current = s ?? {};
      const simple = (s?.simple ?? {}) as Partial<AppSettings>;
      setSettings({ ...DEFAULT_SETTINGS, ...simple });
    }).catch(() => {});
  }, []);
  const update = useCallback((patch: Partial<AppSettings>) => {
    setSettings(prev => {
      const next = { ...prev, ...patch };
      raw.current = { ...raw.current, simple: next };
      api.saveSettings(raw.current).catch(() => {});
      return next;
    });
  }, []);
  useEffect(() => { document.documentElement.dataset.theme = settings.theme; }, [settings.theme]);
  return { settings, update };
}
