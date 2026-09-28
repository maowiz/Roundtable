// Everything about one agent in one place: its name, its model, and its system prompt for each step.
// What is written here is exactly what the model is told; the app adds nothing to it.

import { useEffect, useRef, useState } from 'react';
import { ChevronDown, RotateCcw, Trash2, X } from 'lucide-react';
import type { PanelDoc, SeatDoc, SystemPrompts } from '../api/client';
import { defaultPrompt, MIN_AGENTS, withModel, type ModelOption, type Step } from './state';

const STEP_LABEL: Record<Step, string> = { answer: 'Answer', review: 'Review', improve: 'Improve', synthesize: 'Final answer' };
const STEP_HINT: Record<Step, string> = {
  answer: 'Step 1. What this agent is told when it answers your question alone.',
  review: 'Step 2. What this agent is told when it reviews one other agent’s answer, without knowing whose it is.',
  improve: 'Step 3. What this agent is told when it improves its own answer using the reviews it received.',
  synthesize: 'Step 4. What the final writer is told when it combines the improved answers into one.',
};
const SEAT_FIELD: Record<Exclude<Step, 'synthesize'>, 'promptAnswer' | 'promptReview' | 'promptImprove'> = { answer: 'promptAnswer', review: 'promptReview', improve: 'promptImprove' };

function ModelSelect({ value, options, onPick }: { value: string; options: ModelOption[]; onPick: (o: ModelOption) => void }) {
  const groups = Array.from(new Set(options.map(o => o.providerLabel)));
  const known = options.some(o => o.key === value);
  return <select className="rt-input" value={value} onChange={e => { const o = options.find(x => x.key === e.target.value); if (o) onPick(o); }} aria-label="Model">
    {!known && <option value={value}>{value.split('|')[1] ?? value} (not connected)</option>}
    {groups.map(g => <optgroup key={g} label={g}>{options.filter(o => o.providerLabel === g).map(o => <option key={o.key} value={o.key} disabled={!o.verified}>{o.modelId}{o.verified ? '' : ' (price not verified)'}</option>)}</optgroup>)}
  </select>;
}

export default function AgentEditor({ team, index, options, defaults, focusName, onChange, onDelete, onClose }: {
  team: PanelDoc;
  /** Seat index, or 'writer' for the final writer. */
  index: number | 'writer';
  options: ModelOption[];
  defaults: SystemPrompts | null;
  focusName: boolean;
  onChange: (next: PanelDoc) => void;
  onDelete: (i: number) => void;
  onClose: () => void;
}) {
  const isWriter = index === 'writer';
  const seat = isWriter ? null : team.seats[index];
  const steps: Step[] = isWriter ? ['synthesize'] : ['answer', 'review', 'improve'];
  const [step, setStep] = useState<Step>(steps[0]);
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => { if (focusName && nameRef.current) { nameRef.current.focus(); nameRef.current.select(); } }, [focusName, index]);
  useEffect(() => { setStep(isWriter ? 'synthesize' : 'answer'); }, [index, isWriter]);
  useEffect(() => { const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); }; window.addEventListener('keydown', onKey); return () => window.removeEventListener('keydown', onKey); }, [onClose]);

  if (!isWriter && !seat) return null;
  const setSeat = (patch: Partial<SeatDoc>) => { if (!isWriter) onChange({ ...team, seats: team.seats.map((s, j) => (j === index ? { ...s, ...patch } : s)) }); };
  const modelKey = isWriter ? team.synthesizer.provider + '|' + team.synthesizer.modelId : seat!.provider + '|' + seat!.modelId;
  const opt = options.find(o => o.key === modelKey);
  const pickModel = (o: ModelOption) => {
    if (isWriter) onChange({ ...team, synthesizer: { ...team.synthesizer, provider: o.provider, credentialRef: o.credentialRef, modelId: o.modelId, tuning: { ...team.synthesizer.tuning, temperature: o.supportsTemperature ? team.synthesizer.tuning.temperature : null } } });
    else onChange({ ...team, seats: team.seats.map((s, j) => (j === index ? withModel(s, o) : s)) });
  };
  const prompt = isWriter ? team.synthesizer.prompt : seat![SEAT_FIELD[step as Exclude<Step, 'synthesize'>]];
  const setPrompt = (v: string) => { if (isWriter) onChange({ ...team, synthesizer: { ...team.synthesizer, prompt: v } }); else setSeat({ [SEAT_FIELD[step as Exclude<Step, 'synthesize'>]]: v } as Partial<SeatDoc>); };
  const original = defaultPrompt(defaults, step);
  const name = isWriter ? 'Final writer' : seat!.label;

  const words = isWriter ? team.synthesizer.tuning.maxOutputWords : seat!.answer.maxOutputWords;
  const setWords = (w: number) => {
    if (isWriter) onChange({ ...team, synthesizer: { ...team.synthesizer, tuning: { ...team.synthesizer.tuning, maxOutputWords: w } } });
    else setSeat({ answer: { ...seat!.answer, maxOutputWords: w }, review: { ...seat!.review, maxOutputWords: w }, improve: { ...seat!.improve, maxOutputWords: w } });
  };
  const temp = (v: string) => (v === '' ? null : Math.max(0, Math.min(2, Number(v))));

  return <div className="rt-drawer-backdrop" onMouseDown={e => { if (e.target === e.currentTarget) onClose(); }}>
    <aside className="rt-drawer" role="dialog" aria-label={'Edit ' + name}>
      <header className="rt-drawer-head">
        <div><p className="rt-eyebrow">{isWriter ? 'FINAL WRITER' : 'AGENT'}</p><h2>{name}</h2></div>
        <button className="rt-icon" onClick={onClose} aria-label="Close"><X size={18}/></button>
      </header>

      <div className="rt-drawer-body">
        {!isWriter && <label className="rt-field"><span>Name</span><input ref={nameRef} className="rt-input" value={seat!.label} maxLength={40} onChange={e => setSeat({ label: e.target.value })} placeholder="Give this agent a name" aria-label="Agent name"/></label>}
        <label className="rt-field"><span>Model</span><ModelSelect value={modelKey} options={options} onPick={pickModel}/>
          <small className="rt-hint">Only models from providers with a saved key are listed.{opt ? ` Via ${opt.providerLabel}.` : ''}</small></label>

        <section className="rt-prompts">
          <div className="rt-prompts-head"><h3>System prompt</h3><small>Exactly what this model is told. Edits save by themselves.</small></div>
          {steps.length > 1 && <div className="rt-tabs" role="tablist">{steps.map(s => <button key={s} role="tab" aria-selected={step === s} className={step === s ? 'on' : ''} onClick={() => setStep(s)}>{STEP_LABEL[s]}</button>)}</div>}
          <div className="rt-field">
            <span className="rt-field-row">{STEP_HINT[step]}{prompt !== original && <button className="rt-link" onClick={() => setPrompt(original)}><RotateCcw size={12}/> Reset to default</button>}</span>
            <textarea className="rt-input rt-area rt-prompt" rows={16} value={prompt} onChange={e => setPrompt(e.target.value)} aria-label={'System prompt: ' + STEP_LABEL[step]} spellCheck={false}/>
          </div>
        </section>

        <details className="rt-details">
          <summary><ChevronDown size={14}/> Advanced <em>length, creativity, cost</em></summary>
          <div className="rt-grid3">
            <label className="rt-field"><span>Answer length (words)</span><input className="rt-input" type="number" min={50} max={8000} value={words} onChange={e => setWords(Math.max(50, Math.min(8000, Number(e.target.value) || 50)))}/></label>
            {!isWriter && <label className="rt-field"><span>Cost cap per run ($)</span><input className="rt-input" value={seat!.costCapUsd} onChange={e => setSeat({ costCapUsd: e.target.value })}/></label>}
          </div>
          {opt?.supportsTemperature !== false && <div className="rt-grid3">
            {isWriter
              ? <label className="rt-field"><span>Creativity (0–2)</span><input className="rt-input" type="number" step={0.1} min={0} max={2} value={team.synthesizer.tuning.temperature ?? ''} placeholder="default" onChange={e => onChange({ ...team, synthesizer: { ...team.synthesizer, tuning: { ...team.synthesizer.tuning, temperature: temp(e.target.value) } } })}/></label>
              : (['answer', 'review', 'improve'] as const).map(r => <label key={r} className="rt-field"><span>Creativity: {STEP_LABEL[r]}</span><input className="rt-input" type="number" step={0.1} min={0} max={2} value={seat![r].temperature ?? ''} placeholder="default" onChange={e => setSeat({ [r]: { ...seat![r], temperature: temp(e.target.value) } } as Partial<SeatDoc>)}/></label>)}
          </div>}
          {opt?.supportsTemperature === false && <p className="rt-hint">This model does not accept a creativity setting.</p>}
        </details>
      </div>

      <footer className="rt-drawer-foot">
        {!isWriter ? <button className="rt-btn danger" disabled={team.seats.length <= MIN_AGENTS} title={team.seats.length <= MIN_AGENTS ? 'Keep at least two agents' : undefined} onClick={() => onDelete(index as number)}><Trash2 size={15}/> Delete agent</button> : <span/>}
        <button className="rt-btn primary" onClick={onClose}>Done</button>
      </footer>
    </aside>
  </div>;
}
