# PRD review and research findings

Reviewed 8 September 2026. Scope: the Windows desktop application described in [PRD.md](PRD.md). The user confirmed Windows only. Research and specification work are complete for this revision; no application code was implemented.

## Assessment

The original PRD captured the round-robin idea well, but several statements were too absolute or left contradictory behavior for the developer to resolve. The revised PRD retains independent answers, all-to-all non-self reviews, isolated self-improvement and final synthesis. It makes the rules testable and distinguishes product requirements from unproven quality claims.

## Material changes

| Finding in v0.1 | Revision and reason |
|---|---|
| “Without any model being … steered by another” | Replaced with a precise input-visibility guarantee. Reviews intentionally influence revision; correlated errors and model bias remain possible. |
| “Neutral fifth model” | A separate synthesis role, with actual model reuse disclosed. A fifth role does not prove neutrality. |
| TypeScript engine + SDK/custom fetch did not define a safe execution boundary | Recommend Tauri/React with a Rust controller and provider adapters. Stored keys never return to the renderer. Electron with backend TypeScript remains a documented staffing alternative. |
| Tauri capabilities described as a universal network allow-list | Require native endpoint validation and narrow command authorization too; capabilities do not sandbox arbitrary Rust code. |
| “Quoted content cannot act as instructions” | Describe quoting as one mitigation. Add adversarial tests, schema/lineage validation and inert rendering; models have no operational tools. |
| Retry up to three times plus a separate schema retry | Define three total dispatch attempts, at most one schema retry, one controller and no hidden SDK retry layer. |
| Failed seats silently disappear | Strict P0 completion. Explicit Partial output is separately scoped and always labeled. |
| Crash recovery implied safe request replay | Preserve outcome-unknown calls and disclose possible repeated billing before resending. Exactly-once local acceptance is distinct from provider execution. |
| Append-only delete-blocking triggers conflict with deleting runs | Define normal-workflow immutability and a separate, scoped native deletion path. Explain backup and forensic-erasure limits. |
| “Everything local” underdescribes API privacy | Local history and credentials; task content still goes to chosen providers. The content database is not advertised as an encrypted vault. |
| Pinned model ID assumed reproducibility | Record requested/returned IDs, moving-alias status, settings, prompt/schema/adapter versions and actual ordering. |
| Output schemas lacked robust provenance | Add opaque aliases, issue decisions, claim references and native checks for invalid/foreign IDs. Distinguish lineage from factual verification. |
| Cost estimate was optional | Make preflight estimates, reservations, caps and unknown-billing states P0. Account for repeated input and retries. |
| Universal timing, memory and installer promises | Replace with benchmarked Windows targets and declared measurement conditions; count WebView2 prerequisites separately. |
| Browser UI tests assumed desktop coverage | Separate presentation tests from real Windows command, credential, lifecycle and installer tests. |
| Fixed 14-week schedule without staffing | Replace with milestones and exit criteria; estimates follow feasibility and ownership. |
| “65% better” evaluation lacked a method | Specify held-out tasks, preselected baselines, blind human grading, ties/failures, uncertainty, review precision and cost-matched comparisons. |
| macOS/Linux included in release | Remove from v1 after the user's Windows-only direction. Windows 11 x64 is the proposed test baseline. |

## Research that changed the design

- **Independence and evaluation:** [ICML 2025 correlated-errors research](https://proceedings.mlr.press/v267/kim25e.html) finds shared errors across models; [EMNLP 2025 multi-agent judging research](https://aclanthology.org/2025.findings-emnlp.941/) finds bias amplification in the debate settings it studies. These support caution and testing, not a claim that Roundtable is already superior.
- **Desktop trust boundary:** [Tauri security](https://v2.tauri.app/security/) and [capabilities](https://v2.tauri.app/security/capabilities/) distinguish frontend exposure from privileged native code. The PRD's Rust ownership recommendation follows from that boundary and the desired key isolation.
- **Alternative runtime:** [Electron context isolation](https://www.electronjs.org/docs/latest/tutorial/context-isolation) supports a coherent TypeScript alternative when paired with careful IPC and sandboxing. [Tauri's Node sidecar guide](https://v2.tauri.app/learn/sidecar-nodejs/) confirms that keeping a Node engine outside the renderer adds packaging and lifecycle work.
- **Provider differences:** [AI SDK settings](https://ai-sdk.dev/docs/ai-sdk-core/settings) documents retries and model-specific settings. [Claude](https://platform.claude.com/docs/en/build-with-claude/structured-outputs) and [Gemini](https://ai.google.dev/gemini-api/docs/structured-output) document schema limitations. Therefore a per-provider compatibility gate remains necessary regardless of SDK.
- **Storage and distribution:** [SQLite WAL](https://www.sqlite.org/wal.html), [backup guidance](https://www.sqlite.org/backup.html), [Windows installers](https://v2.tauri.app/distribute/windows-installer/), [code signing](https://v2.tauri.app/distribute/sign/windows/) and [updater signatures](https://v2.tauri.app/plugin/updater/) support the more explicit durability and release requirements.
- **Prompt injection:** [OWASP guidance](https://cheatsheetseries.owasp.org/cheatsheets/LLM_Prompt_Injection_Prevention_Cheat_Sheet.html) supports layered defenses rather than treating prompt delimiters as a hard barrier.

All technical conclusions use official documentation or primary research. No dependency was installed and no paid model test was run. Documentation was checked through web search/opening and Agent Reach's Jina Reader route. Source freshness does not prove library compatibility; actual versions and provider behavior must be validated at M0.

## Readiness and remaining planning inputs

The PRD is ready for architecture review, backlog creation and estimation. It is not a claim that the product or model integrations have already passed their acceptance tests.

Before implementation is scheduled, record engineering ownership, the pilot question domain, model roster, budget/latency profile and Windows signing arrangement. These are named planning decisions in section 20, not missing definitions of how the application behaves.

The original v0.1 is preserved in [archive/PRD-v0.1.md](archive/PRD-v0.1.md). Existing PDF decks were left as historical concept documents; the current Markdown PRD takes precedence for development.

## Subsequent scope amendment

The user subsequently specified a personal app with screens approved before development. PRD v0.3 records that direction. [UI_UX_PLAN.md](UI_UX_PLAN.md) defines seven screens, four navigation destinations and the individual design/approval order. Public-release planning requirements discussed above do not introduce enterprise screens or override that sequence.
