# TRINITY, explained, and what it could give Roundtable

Research note, 2026-09-14. Nothing here is implemented. Cloned for reading only:
`research/fugu` (Sakana AI, official; contains the Fugu technical report), `research/trinity_framework`
(community re-implementation in Elixir, has the role prompts), `research/trinity_paper.pdf` (the ICLR 2026 paper).

## 1. What TRINITY is, in plain words

You have a pool of different AI models (in the paper: GPT-5, Gemini 2.5 Pro, Claude 4 Sonnet and four open
models). Instead of picking one model and asking it the whole question, a very small "manager" model decides,
turn by turn, **which model** should act next and **in which of three roles**:

| Role | What it does | What it hands back |
|---|---|---|
| Thinker | Looks at the question and everything done so far, writes a plan, a breakdown, or a critique | Guidance, no final answer |
| Worker | Does the next concrete step: writes the code, the derivation, the numbers, the draft | Content |
| Verifier | Checks the current solution for correctness and completeness | `ACCEPT` or `REVISE` plus a short diagnosis |

The loop: the manager reads the transcript (the question plus every earlier turn), picks a model and a role,
that model runs with a short role prompt, its output is appended to the transcript, repeat. It stops when a
Verifier says `ACCEPT`, or after a fixed budget of 5 turns. The last output is the answer.

Role prompts are short. The community implementation uses, almost verbatim:

- Thinker: "Analyze the current state and provide high-level guidance, plans, decompositions, or critiques.
  Do not present unchecked final answers unless the transcript already contains enough evidence."
- Worker: "Execute the next concrete step of the plan. Write code, math, derivations, calculations, or concrete
  answer content that advances the solution."
- Verifier: "Check the current solution for correctness, completeness, and responsiveness. Start your response
  with exactly ACCEPT or REVISE. After REVISE, include a concise diagnosis."

## 2. The clever part: the manager is tiny and is trained, not prompted

- The manager is Qwen3-0.6B (a small open model) with a 10K-parameter head bolted on. It never writes text.
  It reads the transcript, and the head turns one internal hidden state into two choices: model and role.
- It is trained by an evolutionary strategy (sep-CMA-ES): try random tweaks of the head, keep the ones that
  produce more correct final answers on training questions, repeat. No gradients, no labels beyond "was the
  final answer right". Training used MATH500, MMLU, RLPR and LiveCodeBench splits.
- Because the manager only emits a decision, it adds almost no latency and almost no cost. The expensive
  calls are the pool models, same as today.

## 3. Results, honestly

- Beats every single model in the pool on the four training benchmarks and on four unseen ones
  (AIME, BigCodeBench, MT-Bench, GPQA-Diamond). Mean relative error reduction of about 22 percent over the
  next best method.
- LiveCodeBench: 86.2 percent pass@1 versus 83.8 for GPT-5 alone.
- On three benchmarks it comes close to the "per-question best" oracle, meaning it mostly learned which
  model is good at which kind of question.
- Ablations that matter to us: removing the Thinker role costs about 4.6 points on the reasoning benchmark;
  forcing a single fixed model costs a lot more. The roles are not decoration.
- Limitation the authors state: the system can plan tool use but cannot act on tools. Also, the manager is
  trained for a specific pool. Swap a model and you retrain.

Sakana's product built on this (Fugu) made a telling simplification: the everyday version **drops the roles**
and only picks the worker, because that is faster. The heavier version (Fugu-Ultra, "Conductor") goes the other
way: a trained orchestrator writes a per-question instruction for each worker and decides who talks to whom.

## 4. How it compares with Roundtable

| | TRINITY | Roundtable |
|---|---|---|
| Shape | Sequential: one model acts per turn, up to 5 turns | Parallel: every seat answers, every seat reviews every other, then improve, then one synthesis |
| Who sees what | Everyone sees the full transcript | Strict isolation: a reviewer sees one answer, an author sees only its own reviews, the writer sees improved answers under aliases |
| Who decides | A trained 0.6B manager picks model and role per turn | A fixed plan decided by the orchestrator; the user picks the seats |
| Stop rule | Verifier says ACCEPT | Fixed four stages always run |
| Cost per question | 1 to 5 model calls | N² + N + 1 calls (21 for four seats) |
| Best at | Getting one correct answer fast on tasks with a checkable result (math, code) | Surfacing disagreement and unsupported claims on open questions where "correct" is not checkable |

Roundtable's isolation is the thing TRINITY does not have and would break: TRINITY's transcript is shared by
design. So Roundtable should not become TRINITY. It can borrow four ideas from it.

## 5. What Roundtable could borrow, ranked by value for cost

1. **A Verifier gate with ACCEPT / REVISE (cheap, high value).** Today the final writer always produces an
   answer. Add one call after synthesis: a Verifier seat reads the question and the final answer only, and
   returns ACCEPT or REVISE with a diagnosis. On REVISE, the final writer gets one more turn with the diagnosis.
   One or two extra calls, a real quality check, and it fits the isolation rules (the verifier sees no papers).
   The verdict would show on the final answer page ("Verified: accepted" or "Revised once after verification").

2. **A Thinker step before the answers (cheap, medium value).** For hard, multi-part questions, one
   Thinker call could produce a plan or breakdown that is given to every seat as reference material. Seats
   still answer independently, they just start from the same map. This is optional per panel and adds one call.
   Risk: a bad plan steers every seat the same way, which reduces the diversity the reviews depend on. Keep it
   off by default and visible in the papers browser when on.

3. **Learned seat selection (expensive, later).** TRINITY's real win is knowing which model to use for which
   question. Roundtable could grow the same knowledge without training anything: the ledger already records
   every run. Over time it can show, per model, how often its claims were judged correct by others and how
   often its improved answer was used by the final writer. A "suggested panel" for a new question could be built
   from those statistics. That is the local, honest version of the trained head, and it needs nothing but the
   data we already keep.

4. **Adaptive stopping (medium cost, medium value).** TRINITY stops when the Verifier is satisfied.
   Roundtable could skip the improvement stage when no review raised an issue, saving N calls on easy
   questions. The rule is simple: if every review of an answer says "no material issue", that seat's improved
   answer is its original answer. No model call needed.

## 6. What not to copy

- Do not share a transcript across seats. It would undo the isolation guarantee, which is the product.
- Do not train a coordinator now. It needs thousands of scored runs per model pool, the pool changes whenever
  the owner swaps a model, and the gain is mostly on benchmarks with a checkable answer.
- Do not adopt the roles as seat types. Every Roundtable seat already plays three roles across the stages
  (answer, review, improve). The Thinker and Verifier are better added as two small optional stages.

## 7. Suggested order, if this is pursued

1. Verifier gate on the final answer (one or two calls, one afternoon of work, visible on the result page).
2. Skip-improve-when-clean rule (saves money on easy questions, no new calls).
3. Optional Thinker plan stage, off by default.
4. Per-model track record in History, then a suggested panel from it.

Sources: [arXiv 2512.04695](https://arxiv.org/abs/2512.04695) (ICLR 2026), [Sakana AI blog](https://sakana.ai/trinity/),
[SakanaAI/fugu](https://github.com/SakanaAI/fugu) with the Fugu technical report,
[nshkrdotcom/trinity_framework](https://github.com/nshkrdotcom/trinity_framework) (community implementation, role prompts),
[Fugu orchestration explainer](https://cahidarda.com/articles/sakana-fugu).
