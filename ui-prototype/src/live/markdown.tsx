// Small, safe Markdown renderer for model answers: headings, lists, rules, paragraphs, bold/italic/code.
// Produces React elements only (never HTML), so a model can not inject markup.

import type { ReactNode } from 'react';

function inline(text: string, key = 0): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /(\*\*[^*]+\*\*|`[^`]+`|\*[^*\n]+\*|_[^_\n]+_)/g;
  let last = 0; let m: RegExpExecArray | null; let i = key * 1000;
  while ((m = re.exec(text))) {
    if (m.index > last) out.push(text.slice(last, m.index));
    const tok = m[0];
    if (tok.startsWith('**')) out.push(<strong key={i++}>{tok.slice(2, -2)}</strong>);
    else if (tok.startsWith('`')) out.push(<code key={i++}>{tok.slice(1, -1)}</code>);
    else out.push(<em key={i++}>{tok.slice(1, -1)}</em>);
    last = m.index + tok.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

export function Markdown({ text }: { text: string }) {
  const lines = String(text ?? '').replace(/\r/g, '').split('\n');
  const blocks: ReactNode[] = [];
  let para: string[] = []; let list: { ordered: boolean; items: string[] } | null = null; let k = 0;
  const flushPara = () => { if (para.length) { blocks.push(<p key={k++}>{inline(para.join(' '), k)}</p>); para = []; } };
  const flushList = () => { if (list) { const items = list.items.map((it, i) => <li key={i}>{inline(it, k * 100 + i)}</li>); blocks.push(list.ordered ? <ol key={k++}>{items}</ol> : <ul key={k++}>{items}</ul>); list = null; } };
  for (const raw of lines) {
    const line = raw.trimEnd();
    const t = line.trim();
    if (!t) { flushPara(); flushList(); continue; }
    const h = /^(#{1,6})\s+(.*)$/.exec(t);
    if (h) { flushPara(); flushList(); const lvl = Math.min(4, h[1].length + 1); const Tag = (`h${lvl}`) as 'h2' | 'h3' | 'h4'; blocks.push(<Tag key={k++}>{inline(h[2], k)}</Tag>); continue; }
    if (/^(-{3,}|\*{3,}|_{3,})$/.test(t)) { flushPara(); flushList(); blocks.push(<hr key={k++}/>); continue; }
    const ul = /^[-*•]\s+(.*)$/.exec(t);
    const ol = /^\d+[.)]\s+(.*)$/.exec(t);
    if (ul || ol) {
      flushPara();
      const ordered = !!ol;
      if (!list || list.ordered !== ordered) { flushList(); list = { ordered, items: [] }; }
      list.items.push((ul ?? ol)![1]);
      continue;
    }
    if (list && /^\s{2,}/.test(raw)) { list.items[list.items.length - 1] += ' ' + t; continue; }
    flushList();
    para.push(t);
  }
  flushPara(); flushList();
  return <div className="md">{blocks}</div>;
}

/** First sentence or so, for collapsed cards. */
export function excerpt(text: string, words = 34): string {
  const plain = String(text ?? '').replace(/[#*`_>-]+/g, ' ').replace(/\s+/g, ' ').trim();
  const ws = plain.split(' ');
  return ws.length > words ? ws.slice(0, words).join(' ') + '…' : plain;
}
