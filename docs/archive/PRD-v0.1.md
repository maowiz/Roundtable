# Roundtable AI — Product Requirements Document

| | |
|---|---|
| Status | Draft for review |
| Version | 0.1 |
| Date | 8 September 2026 |
| Product | Roundtable AI desktop app |
| Chosen stack | Tauri 2 · React + TypeScript · TypeScript engine on Vercel AI SDK 6 · SQLite · OS keychain |

## 1. Summary

Roundtable AI is a desktop application that answers one question with several large language models at once, then makes them review each other without any model being able to copy, anchor on, or be steered by another. Four models answer privately, each answer is reviewed by the other three in fresh anonymous sessions, each model improves only its own answer, and a fifth neutral model writes the final answer. Every paper produced along the way is kept in a local, append-only ledger so the user can see exactly who said what and why the final answer looks the way it does.

The product's core value is not the interface or the vendor wrapper. It is the memory-isolation rule, enforced in code, and the provenance ledger that proves it was followed.

## 2. Problem

- A single model is confidently wrong often enough that people re-ask the same question in two or three chat apps and compare by hand.
- Existing multi-model tools put the models in one shared conversation. The first answer anchors the rest, verbose answers get favoured, and a wrong idea snowballs. Research on multi-agent debate shows these biases grow after the first round.
- Nobody can audit a group chat afterwards to see which model contributed what.

## 3. Goals

1. Produce a final answer that is measurably better than the best single model's first answer on a fixed question set.
2. Guarantee, by construction, that no model sees another model's work at any stage where it could bias its own.
3. Show the whole round table live so a non-technical viewer understands what happened.
4. Keep everything local: keys in the OS keychain, ledger on disk, no server of ours in the middle.
5. Stay flexible: any number of seats, any vendor per seat, a system prompt per seat, extra review rounds.

## 4. Non-goals for version 1

- Tool use, web search or file reading by the panel models. Text-in, text-out only.
- Agents talking to each other directly. There is no chat between seats, ever.
- Team or cloud sync. One user, one machine.
- Mobile builds, even though Tauri can produce them.
- Fine-tuning, local model hosting, or a model marketplace.

## 5. Users

| Persona | Need | Success looks like |
|---|---|---|
| Analyst or researcher | High-stakes answers with visible reasoning and disagreement | Reads the final answer, opens the ledger, trusts it |
| Engineering lead evaluating LLMs | Compare vendors on the same prompt without bias | Exports a run and its per-model scores |
| Executive sponsor | Understand what the tool does in one screen | Watches one live run and can explain it afterwards |

## 6. Product principles

- **Isolation is a property of the code, not of the prompt.** We never ask a model to forget. We never give it the thing in the first place.
- **The controller owns memory.** Models are stateless workers. All memory is the ledger, and only the controller reads it.
- **Every call is a sealed envelope.** The envelope for each call is logged, so anyone can prove what each model saw.
- **Anonymous by default.** Reviewers never learn which vendor wrote the answer in front of them.
- **Show, don't summarise.** The interface renders the actual papers, not a paraphrase of them.

## 7. The workflow

One run of the round table, with the default four seats.

| Stage | Calls | Runs in parallel | Envelope contents |
|---|---|---|---|
| 1. Answer | 4 | Yes | Seat system prompt + the question |
| 2. Review | 12 | Yes | Reviewer system prompt + the question + one anonymous answer |
| 3. Improve | 4 | Yes | Seat system prompt + the question + its own first answer + the three reviews written about it |
| 4. Synthesise | 1 | n/a | Synthesiser system prompt + the question + the four improved answers, shuffled |

Total: 21 calls. The user experiences four waits, not twenty-one.

For N seats the count is N + N(N−1) + N + 1.

## 8. Functional requirements

Priority: P0 must ship in v1. P1 should ship in v1. P2 is planned for later.

### 8.1 Setup and keys

| ID | Requirement | Priority |
|---|---|---|
| FR-1 | On first launch the app guides the user to add at least two vendor API keys. | P0 |
| FR-2 | Keys are stored only in the OS credential store (Windows Credential Manager, macOS Keychain, Linux Secret Service). Never in config files, logs, or the ledger. | P0 |
| FR-3 | The user can test a key with one cheap call and see success or the vendor's error text. | P0 |
| FR-4 | Keys can be removed. Removal is immediate and complete. | P0 |

### 8.2 Panel configuration

| ID | Requirement | Priority |
|---|---|---|
| FR-5 | A panel is a named list of seats. Default panel has four seats on four different vendors. | P0 |
| FR-6 | Each seat has: display label, vendor, exact pinned model id, temperature, max output tokens, answerer system prompt, reviewer system prompt, improver system prompt. | P0 |
| FR-7 | The synthesiser is a separate seat with its own vendor, model and system prompt. It defaults to a model not on the panel. | P0 |
| FR-8 | Panel size is any integer from 2 to 8. | P0 |
| FR-9 | Panels can be duplicated, renamed, exported and imported as JSON. | P1 |
| FR-10 | Number of review rounds is configurable from 1 to 3. Each extra round repeats stages 2 and 3 on the improved answers. | P2 |

### 8.3 Asking a question

| ID | Requirement | Priority |
|---|---|---|
| FR-11 | The user types a question, picks a panel, and starts a run. | P0 |
| FR-12 | Before the run starts the app shows the estimated call count and an estimated cost from the vendors' published prices. | P1 |
| FR-13 | A run can be cancelled. Calls already finished are kept in the ledger and the run is marked cancelled. | P0 |

### 8.4 The engine

| ID | Requirement | Priority |
|---|---|---|
| FR-14 | Every model call is a brand-new request with no prior messages. The engine never sends a conversation id, thread id, or previous-response id. | P0 |
| FR-15 | The engine builds each envelope from the ledger using exactly the recipe in section 7 and nothing else. | P0 |
| FR-16 | Each envelope is written to the ledger before the call is made, with the ids of every paper it contains. | P0 |
| FR-17 | A reviewer never receives its own answer. The engine skips the diagonal of the review matrix. | P0 |
| FR-18 | Answers shown to reviewers and to the synthesiser carry no vendor, model or seat name. | P0 |
| FR-19 | Improved answers are shuffled with a recorded random seed before synthesis. | P0 |
| FR-20 | All calls within a stage run concurrently, with a configurable concurrency cap per vendor. | P0 |
| FR-21 | A failed call is retried up to three times with backoff. After that the seat is marked failed for that stage; the run continues with the remaining seats and the final answer states which seats were missing. A failed seat is never silently replaced by another model. | P0 |
| FR-22 | Answers, reviews and improved answers are requested as structured output and validated against a schema. A response that fails validation is retried once, then stored raw and flagged. | P0 |
| FR-23 | Candidate answers are wrapped as quoted, untrusted content inside review and synthesis prompts, so text inside an answer cannot act as an instruction to the next model. | P0 |

### 8.5 The round-table view

| ID | Requirement | Priority |
|---|---|---|
| FR-24 | The main screen is a round table with one chair per seat and the question in the centre. | P0 |
| FR-25 | Each stage is shown as it happens: seats thinking, papers appearing on chairs, papers rotating, reviews arriving, seats rewriting, the synthesiser working. | P0 |
| FR-26 | Model output streams into its paper as tokens arrive. | P1 |
| FR-27 | Clicking any paper opens it in full, with its envelope: exactly what that call was given. | P0 |
| FR-28 | The final answer is shown with a provenance panel: which improved answers it drew on and which points remain disputed. | P0 |

### 8.6 History and export

| ID | Requirement | Priority |
|---|---|---|
| FR-29 | Every run is saved. A history list shows question, panel, date, cost and status. | P0 |
| FR-30 | A run can be exported as a self-contained HTML file or as JSON containing every paper and envelope. | P1 |
| FR-31 | A run can be replayed in the round-table view at any speed. | P2 |
| FR-32 | The user can delete a run. Deletion removes all of its papers. | P0 |

### 8.7 Settings

| ID | Requirement | Priority |
|---|---|---|
| FR-33 | Light and dark theme following the OS by default. | P0 |
| FR-34 | Signed automatic updates with a release note shown before install. | P1 |
| FR-35 | An optional custom base URL per vendor for corporate gateways. | P1 |

## 9. The isolation contract

These rules are requirements, not guidance. Each one has an automated test.

| Stage | May see | Must not see |
|---|---|---|
| Answer | Question, own system prompt | Any other seat's answer |
| Review | Question, one anonymous answer, reviewer prompt | Its own answer, the other answers, any earlier review, the author's identity, any chat history |
| Improve | Question, its own first answer, the reviews written about it, improver prompt | Other seats' answers, the reviews it wrote for others |
| Synthesise | Question, the improved answers shuffled, synthesiser prompt | Raw reviews, first answers, chat history, seat or vendor names |

Leak testing: the test suite plants a unique canary string in each paper and asserts that no envelope contains a canary it is not entitled to. This runs in CI on every commit.

## 10. Non-functional requirements

| Area | Requirement |
|---|---|
| Performance | Cold start under 1 second. Idle memory under 100 MB. Installer under 15 MB on Windows. |
| Latency | A four-seat run completes in roughly the time of the four slowest calls in sequence, not twenty-one. |
| Reliability | A crash mid-run loses nothing already written to the ledger. On restart the run is shown as interrupted and can be resumed from the last completed stage. |
| Security | Keys in the OS keychain only. Outbound network limited to the configured vendor hosts through the Tauri capability allowlist. No telemetry leaves the machine. |
| Privacy | Questions and papers never leave the machine except in the calls to the vendors the user configured. |
| Accessibility | Keyboard navigation for every action. Text contrast meets WCAG AA in both themes. |
| Platforms | Windows 10+ and macOS 13+ at v1. Linux best-effort. |

## 11. Technical design

### 11.1 Stack

| Layer | Choice | Reason |
|---|---|---|
| App shell | Tauri 2 | Small installer, low memory, capability-based security, native installers for all three desktops |
| UI | React 19, TypeScript, Vite | Largest talent pool; the same UI can ship on the web later |
| Engine | `@roundtable/engine`, a pure TypeScript package with no UI dependency | Portable to a server later without rewriting |
| Model calls | Vercel AI SDK 6 with direct provider packages (`@ai-sdk/openai`, `@ai-sdk/anthropic`, `@ai-sdk/google`, `@ai-sdk/xai`) | One API, structured output, streaming. Vendors configured directly, not through the Vercel gateway |
| Networking | `@tauri-apps/plugin-http` passed as the SDK's custom `fetch` | Avoids webview cross-origin blocks; hosts allow-listed in the capability file |
| Ledger | SQLite through `@tauri-apps/plugin-sql` | One local file, migrations built in, append-only enforced by triggers |
| Secrets | `keyring` crate behind a small Tauri command | OS credential store on all three platforms |
| Updates | `@tauri-apps/plugin-updater` | Signed releases |
| Testing | Vitest for the engine, Playwright for the UI, a canary leak suite in CI | |

### 11.2 Components

- **Engine.** Exposes `runRoundTable(question, panel, deps)`. Deps are `callModel`, `ledger`, `random`, `clock`, so the engine is testable with fakes.
- **Envelope builder.** One function per stage. Takes the ledger and returns the exact message list for one call. This is the only code allowed to read the ledger on the engine's behalf.
- **Ledger.** Append-only tables `runs`, `papers`, `envelopes`, `envelope_papers`, `calls`. Update and delete triggers raise errors on `papers` and `envelopes`.
- **Vendor adapter.** Thin wrapper over the AI SDK that adds the concurrency cap, retry, timing and cost capture.
- **UI state.** The engine emits events (`stage_started`, `paper_created`, `token`, `call_failed`, `run_finished`). The round-table view subscribes and animates from events, never from polling.

### 11.3 Data model

```
runs        id, question, panel_json, status, started_at, finished_at, cost_usd
papers      id, run_id, kind (answer|review|improved|final), seat_id, about_paper_id, body_json, created_at
envelopes   id, run_id, stage, seat_id, system_prompt, messages_json, created_at
envelope_papers  envelope_id, paper_id
calls       id, envelope_id, vendor, model, tokens_in, tokens_out, latency_ms, cost_usd, error
```

### 11.4 Paper schemas

Answer: `answer`, `assumptions[]`, `key_claims[]`, `confidence` 0–1.

Review: `correct_points[]`, `problems[] {text, severity: low|medium|high}`, `missing[]`, `proposed_fix`, `confidence`, `needs_external_verification` boolean.

Improved answer: `accepted_criticisms[]`, `rejected_criticisms[] {text, reason}`, `answer`, `confidence`.

Final: `answer`, `sources_by_claim[] {claim, from_improved_ids[]}`, `unresolved[]`, `confidence`.

### 11.5 Prompts

Four prompt roles, versioned, stored with each panel: answerer, reviewer, improver, synthesiser. Reviewer prompts ask for verification of claims, not judgement of writing style. The synthesiser prompt forbids majority voting as a substitute for correctness and requires listing unresolved disagreement.

## 12. Interface outline

1. **Ask.** Question box, panel picker, cost estimate, Start.
2. **Round table.** Live stage view with the chairs, papers and rotation. Stage strip across the top. Cancel button.
3. **Result.** Final answer left, provenance panel right, four improved answers below as tabs.
4. **Paper viewer.** Any paper plus its envelope, side by side.
5. **Panels.** Seat editor with prompt fields and a test button per seat.
6. **History.** Searchable list of runs.
7. **Settings.** Keys, theme, updates, gateways.

## 13. Success metrics

| Metric | Target for v1 |
|---|---|
| Final answer beats best single first answer on the internal 100-question set | 65% of questions or more, graded blind |
| Reviews that identify at least one real error in a seeded-error test | 80% or more |
| Canary leak tests | Zero failures, every commit |
| Median run time, four seats, ordinary question | Under 90 seconds |
| Crash-free runs | 99% or more |

## 14. Milestones

| Milestone | Scope | Target |
|---|---|---|
| M0 Engine proof | Engine package, fake vendors, ledger, canary suite green | Week 3 |
| M1 Internal alpha | Real vendors, keys in keychain, basic round-table view, history | Week 7 |
| M2 Beta | Streaming, paper viewer with envelopes, export, cost estimate, updater | Week 11 |
| M3 v1.0 | Polish, accessibility pass, signed installers for Windows and macOS | Week 14 |

## 15. Risks

| Risk | Mitigation |
|---|---|
| Vendor rate limits stall the 12-review stage | Per-vendor concurrency cap; stage still completes, just slower |
| Vendor changes or retires a pinned model | Health check at run start; the user is asked to pick a replacement, never auto-swapped |
| Structured output differs per vendor | The AI SDK normalises it; schema validation plus one retry catches the rest |
| Reviews reward long or confident answers | Equal token limits per seat; reviewer prompt scores claims, not style |
| A model treats text inside an answer as an instruction | Answers are quoted as untrusted data in every downstream prompt |
| Webview differences across Windows and macOS | Playwright runs on both in CI |

## 16. Open questions

1. Should the synthesiser be allowed to see the raw reviews as well as the improved answers? Default is no, to keep it simple and reduce leakage paths.
2. Do we hide vendor names in the final provenance panel from the user too, or only from the models? Default: models only; the user sees everything.
3. Should a seat be allowed to be the same model twice with different prompts? Default: allowed, warned.

## 17. Glossary

- **Seat.** One configured model with its prompts. A chair at the table.
- **Paper.** Any output: answer, review, improved answer or final.
- **Envelope.** The exact input given to one model call.
- **Ledger.** The local append-only store of runs, papers and envelopes.
- **Canary.** A unique marker planted in a paper to detect leaks in tests.
