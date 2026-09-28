# Roundtable diagrams, and how to explain each one

Five interactive diagrams made with archify. Open any `.html` in a browser: Light/Dark button, pan and zoom,
"Play story" walks the guided views, Export gives PNG/SVG for slides. The `.json` next to each is the source.
Light PNG snapshots at 1440×900 sit beside each file (`*.visual-check.1440x900.light.png`).

| File | Type | Summary |
|---|---|---|
| roundtable-architecture.html | Architecture | The parts of the app and how they connect |
| roundtable-run.html | Workflow | What happens in one run, lane by lane |
| roundtable-data.html | Data flow | Where your question, key and money go, and what stays on the PC |
| roundtable-states.html | Lifecycle | The states a run can be in, and how it pauses, resumes and ends |
| roundtable-call.html | Sequence | One model call, step by step, from ledger to provider and back |

---

## 1. Architecture: the parts

**What it shows.** Boxes are components, arrows are who talks to whom. The dashed box is the Rust core; everything
inside it is compiled code with automated tests. Outside are you, the display, and the internet.

**How to read it, left to right.** You ask. The Renderer only displays. The Tauri host is a locked front door with
22 validated commands. The Engine is the conductor of the four stages. Provider adapters translate to each provider's
API. OpenRouter is where the four models live.

**Top row, the isolation machinery.** The Envelope builder packs, for every call, only what that stage may see, with
names replaced by aliases. The Paper validator rejects any reply that is not a properly structured paper.

**Bottom row, money and records.** The Ledger is one SQLite file where every run, call, attempt and payment is an
unchangeable row. Budget reserves the maximum cost before a call and settles the real cost after. The Price catalog
holds verified per-token prices.

**Top left, keys.** Secrets reads the key only at the moment of sending and wipes it. Credential Manager is Windows'
own vault where the key lives. It is never in a file.

**One sentence.** "Your question goes through a locked door to a conductor that seals an envelope for every AI call,
checks every reply, writes everything to an unchangeable ledger, and never lets a model see more than its stage allows."

---

## 2. Workflow: one run

**What it shows.** Four lanes: You, the Deliberation stages, Budget & recovery, and the Ledger. Time runs left to right.

**Main path.** Question → Preflight (keys, prices, cost estimate) → Independent answers (N calls, each seat alone) →
Peer reviews (N×(N−1) calls, each reviewer sees one answer under an alias) → Self-improvement (each seat sees only its
own answer and the reviews about it) → Final synthesis (one writer, improved answers shuffled) → Final answer.

**The alias labels on the arrows.** A- for answers, R- for reviews, C- for candidates. No model ever sees a model name.

**Budget lane.** Before every call, the maximum cost is reserved. After the reply, the real cost is settled. Cut-off or
rate-limited calls retry up to three times with more room. When attempts run out or a reply is uncertain, the run
blocks and waits for you. Resume never re-sends what succeeded.

**Ledger lane.** Every call commits its envelope and attempts. Only accepted papers are stored. Web sources are kept
when search was on.

**One sentence.** "Answer alone, review everyone else, improve your own, then one writer combines. Twenty-one fresh
requests for four seats, every one recorded."

---

## 3. Data flow: where things go

**What it shows.** Five stages from Inputs to What you see. Green arrows are the main data, red dashed arrows are
secrets and policy, purple dashed arrows are asynchronous.

**Inputs.** Your question and context. The panel (seats, prompts, caps). The API key, which lives in Credential Manager.

**Prepare.** Envelopes, one per call, aliased. The price catalog produces the cost estimate before anything is sent.

**Outside the PC.** OpenRouter, the only thing that leaves the machine: one envelope at a time over HTTPS, with the key
as a header on that single request.

**Check + store.** Replies are untrusted until they pass the paper schema. Accepted papers and their digests go into
the ledger, along with real token counts turned into settled cost.

**What you see.** The final answer with claims and sources, and History, which reads the ledger.

**One sentence.** "Only sealed envelopes leave the PC. The key, the records and the answers stay."

---

## 4. Lifecycle: the states of a run

**What it shows.** Top row: the states a healthy run passes through. Middle row: paused states you can resume.
Bottom row: how a run ends.

**Healthy path.** Validating → Answering → Reviewing → Improving → Synthesizing → Completed. The engine only moves
forward when every call of a stage is accepted. Each move is one committed row plus one event, so History can replay it.

**Paused.** The app stops on purpose when the ceiling would be exceeded, when a call fails three times, or when a reply
is uncertain (the provider may have billed but never answered). It also pauses if the app is closed mid-run. In all
cases, Resume continues from the ledger and nothing that succeeded is sent again.

**Cancelling.** Pressing stop waits for in-flight requests to settle so every cent is accounted for, then ends as
Cancelled.

**Failed.** Reserved for storage or configuration errors, not model errors.

**One sentence.** "A run only moves forward on proof, pauses instead of guessing, and can always be resumed."

---

## 5. Sequence: one model call

**What it shows.** Eight participants across the top; time runs downward. Each arrow is one step of a single call,
and the dashed bands are the ledger transactions.

**tx1, envelope and ready.** The engine loads only the committed papers this stage may see, builds the envelope with
aliases and a digest, and commits it.

**tx2, reserve and intent.** The budget reserves the maximum cost. If the ceiling would be crossed, the run blocks here,
before anything is sent. The attempt intent is committed.

**Wire.** The key is read from Credential Manager, the adapter sends one HTTPS request in JSON mode, the key is wiped.
The provider replies with the paper, token usage and any web citations.

**tx3, outcome, paper, settle.** The validator checks the reply against the schema; an invalid reply triggers one
schema retry. Real tokens (plus the search fee, if used) are settled. Outcome, paper and settlement are committed
together, and the UI updates from that event.

**One sentence.** "Every call is prepared from the ledger, paid for before sending, checked after, and committed as
one unit, so the record and the money always agree."
