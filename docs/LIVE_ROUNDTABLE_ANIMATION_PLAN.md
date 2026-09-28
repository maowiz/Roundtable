# Live Roundtable — animation and interaction plan

Status: implemented in the design prototype on 8 September 2026 (ui-prototype/src/live). Notes on what changed from the plan are in section 9.
Scope: the Live Roundtable screen (S03) in the design prototype, built so the real engine can drive it later without changes.

## 1. Principle: one event timeline drives everything

The table is a stage. Every visual change is triggered by a named event. The demo plays a pre-recorded, timed event script. The real engine will emit the same events from live model calls. Replays from History reuse the same player.

Event vocabulary (engine and demo share it):

| Event | Payload | Visual |
|---|---|---|
| run_started | question, seats[] | question drops into centre, ripple to chairs |
| seat_thinking | seat | glow, progress ring, streaming lines |
| token | seat, text | text lines grow |
| paper_created | seat, paperId, kind (answer / improved / final) | paper slides out and docks on chair |
| rotation_started | index, mapping seat→chair | avatars glide one chair clockwise |
| envelope_sent | seat, chair, paperId | envelope flies centre → chair, opens |
| review_started | reviewer, paperId | magnifier sweep over the paper |
| review_done | reviewer, paperId | coloured tag flies onto the paper stack |
| context_cleared | seat | memory bar wipes clean |
| seat_returned | seat | avatar glides home |
| improve_started | seat | paper + tags fold in, seat pulses |
| shuffle | order[] | improved papers slide to centre and visibly shuffle |
| synthesis_started | writer | papers merge under the final writer avatar |
| run_finished | finalPaperId | final paper expands into the Final Answer screen |
| call_failed | seat, stage | chair dims, red tag, seat marked missing |

## 2. Flexible seats (2 to 8)

- Polar layout: angle(i) = −90° + i × 360° / N; radius derived from container size.
- Card scale by count: N ≤ 4 full card; 5–6 compact card; 7–8 avatar + status only, details on hover.
- Add seat: ghost "+" chair on the ring; new seat springs in, others re-space.
- Remove seat: X on hover; seat fades out, ring re-spaces.
- Reorder: drag along the ring.
- Papers and review tags are keyed by seat id, so add/remove never breaks a run in progress (editing is only allowed before a run starts).
- Review count shown live: N × (N − 1).

## 3. Storyboard

1. **Ask.** Question card drops into the centre. Ripple spreads outward, lighting each chair in order.
2. **Independent answers.** All seats think at once: breathing glow, avatar progress ring, streaming skeleton lines that become text. Paper slides out and docks. No connecting lines: nobody talks to anybody.
3. **Rotate and review.** Avatars lift and glide one chair clockwise; papers stay. Per chair: envelope flies from centre controller, opens; magnifier sweep; review tag in reviewer's colour flies back onto the paper stack. Between rotations: context_cleared wipe on the reviewer's memory bar. Centre shows "Rotation k of N−1" with a filling arc. After N−1 rotations each paper has N−1 tags.
4. **Return and improve.** Avatars glide home. Paper and tags fold into the chair, seat pulses, "v2" paper emerges with a shimmer along changed lines.
5. **Final synthesis.** Improved papers slide to centre, shuffle visibly ("order randomised"), merge into one paper under the final writer avatar, which expands into the Final Answer screen.

Cinematic vs true timing: real reviews run in parallel. Default "cinematic" plays rotations sequentially for the story; a "true timing" toggle plays events at their recorded times. The activity list on the right always shows the real order.

## 4. Interaction

- Hover seat: trail lights up (its paper, incoming and outgoing review arcs).
- Click seat: side panel with streaming text, exact envelope, tokens, time, cost.
- Click paper stack: fan out reviews; click a tag to read it.
- ~~Timeline scrubber: play/pause, speed 0.5×–4×, step, drag, jump to stage.~~ Removed 2026-09-10: the scrubber read as a video player, not an application. The sample keeps a plain "Play sample" text control; a live run shows a status line only. Stage strip buttons jump (sample) or open that stage's papers (live run).
- Views: Table / Matrix / List toggle. Matrix fills cell by cell; recommended at N ≥ 7.
- Keyboard: Space play/pause, ←/→ step, 1–4 stage, Esc close.
- Reduced motion: movement becomes crossfade; timing unchanged.

## 5. Technical approach

- Framer Motion (motion/react) for springs, shared-layout moves and AnimatePresence add/remove.
- SVG layer for rings, arcs, flight paths (envelopes and tags follow paths).
- CSS for glows and pulses. Only transform and opacity animate.
- State: a reducer holding seats, papers, tags, avatar→chair mapping, stage, timeline position, speed. Pure function `layout(i, N, size)` for positions.
- Demo script: JSON array of `{t, event, payload}`; player advances a controllable clock so the capture script can step deterministically.
- Same components and event names will be reused by the Tauri app; the engine's event emitter replaces the demo player.

## 6. Glass surfaces (acrylic translucency)

The look is Windows 11 **Acrylic** (blurs whatever is behind the window, tinted) on the sidebar, with Mica-style tinting elsewhere. It is drawn by the OS compositor, not by CSS. We adopt the same look in two layers so the prototype and the real app match.

### 6.1 Where glass is used

| Surface | Glass? | Notes |
|---|---|---|
| Left sidebar | Yes, primary | Primary glass surface; desktop or wallpaper shows through blurred |
| Right "What's happening" panel | Yes | Lighter tint than the sidebar |
| Dialogs and sheets (seat editor, paper viewer) | Yes | Backdrop dims and blurs the table behind |
| Timeline scrubber bar | Removed | Replaced by the status line and the stage paper browser (`StageDocs.tsx`) |
| Seat detail side panel | Yes | Slides in over the table |
| Toasts | Yes | |
| Seat cards, papers, tags on the table | No blur | They animate; blur on moving elements costs frames. Solid surface with 92–96% opacity and a 1px highlight edge instead |
| Main content ground | Mica-style tint | Faint wallpaper tint, no blur |

### 6.2 Recipe

Design tokens (light / dark):

- glass-tint: rgba(255,255,255,0.62) / rgba(20,24,31,0.55)
- glass-blur: 28px, saturate 140%
- glass-edge: 1px inset highlight rgba(255,255,255,0.55) / rgba(255,255,255,0.08)
- glass-shadow: 0 12px 40px rgba(16,24,40,0.10) / 0.35
- grain: 3% noise overlay so large flat areas do not band

CSS for any glass surface:

    background: var(--glass-tint);
    backdrop-filter: blur(var(--glass-blur)) saturate(140%);
    -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(140%);
    box-shadow: inset 0 0 0 1px var(--glass-edge), var(--glass-shadow);

**Prototype (Edge app window).** The OS cannot blur through a web page, so the app paints its own "desk": a soft, slowly drifting mesh gradient with a faint grain behind everything. Glass panels blur that desk. This looks the same as the real thing on screen share and costs nothing to set up.

**Real app (Tauri 2).** Window set to transparent with `windowEffects: { effects: ["acrylic"] }` in tauri.conf.json (Mica as fallback when Acrylic is unavailable); `html, body { background: transparent }`. The OS then blurs the desktop behind the sidebar like other Windows 11 apps, and the same CSS above handles the inner panels. The `window-vibrancy` crate offers the same via `apply_acrylic` and `apply_mica` if finer control is needed.

### 6.3 Rules and fallbacks

- Contrast first: text on glass must still meet WCAG AA. Tint alpha never below 0.55. Test on a busy wallpaper.
- Never put backdrop-filter on an element that moves or scales; blur only static panels.
- At most 3 blurred layers on screen at once. Blur radius fixed, never animated.
- Honour `prefers-reduced-transparency` and the reduced-motion setting: switch to solid surfaces.
- Settings gets a "Translucent surfaces" toggle (on by default); off gives solid surfaces.
- Windows 10 and Windows 11 with Energy Saver on do not draw Acrylic; the app detects this and uses the in-app desk fallback so the look survives.
- Known Windows quirk: Acrylic can stutter while dragging the window on some builds; Mica is the fallback material for the base window.

## 7. Build order

0. Glass surfaces: desk background, glass tokens, sidebar, right panel, dialogs, scrubber, Settings toggle, fallbacks. Applied app-wide, so every screen benefits.
1. Polar layout, add/remove/reorder seats, reduced-motion support.
2. Event timeline, player, scrubber, demo script.
3. Stage 1: thinking, streaming, paper docking.
4. Stage 2: rotation, envelopes, sweep, tags, context wipe.
5. Stage 3: return and improve.
6. Stage 4: shuffle and synthesis into Final Answer.
7. Hover trails, click panels, Matrix view, keyboard, failure state.
8. Polish, dark mode, screenshots, refreshed demo build, plan and PRD updates.

Estimate: steps 0–3 one session; 4–6 two sessions; 7–8 one session.

## 8. Decisions to confirm

- AIs move, papers stay (matches the concept); clockwise rotation.
- Cinematic default with a true-timing toggle.
- Maximum 8 seats.
- Framer Motion as the animation library (MIT).
- Glass on by default with a Settings toggle; Acrylic in the Tauri app with Mica and in-app desk as fallbacks.

## 9. Implementation notes

- **No animation library.** Instead of Framer Motion the table is rendered as a pure function of the timeline position: every frame derives the whole world (seat status, avatar positions, flights, tags, shuffle) from the event script and the current time. Scrubbing backwards, stepping and replays came for free, and the screenshot tests are deterministic. Framer Motion remains an option for the Tauri build if richer enter/leave springs are wanted.
- **Ellipse, not circle.** Chairs sit on an ellipse whose width follows the container and whose height follows the table height, so 2 to 8 seats use the space well. Tokens (the AIs) orbit an inner ellipse.
- **Context wipe** is shown on the token's memory bar plus an amber "Clearing context…" status on the chair; the floating label was dropped because it collided with neighbouring cards at six or more seats.
- **Glass** is implemented as planned: desk gradient, `.glass` surfaces (window bar, sidebar, top bar, right panel, dialogs, scrubber), a Settings toggle, and reduced-transparency and reduced-motion fallbacks.
- Files: `src/live/script.ts` (events, script builder, derive), `src/live/Roundtable.tsx` (table, tokens, flights, centre stage, matrix, list, seat panel, scrubber, keyboard), `src/live/live.css`.

## Update 2026-09-10: stage paper browser replaces the timeline

- The play button and timeline are gone from both the sample and live runs. No control on the Roundtable screen resembles a media player.
- The four stage boxes are buttons. On a live or completed run they open a page with that stage's committed papers: independent answers (one card per seat), peer reviews (grouped by whose answer, each card labelled "Written by X · about Y's answer"), improved answers (with how each seat used its reviews), and the final synthesis. Previous and Next buttons walk the four pages; a "Papers" view toggle sits beside Table, Matrix and List.
- Papers come from a new host command `list_run_papers(run_id)` that returns validated bodies from the ledger; every card links to the envelope inspector.
- Seats served through one gateway (OpenRouter) get per-seat badges (model initial, colour by seat) so the review labels are readable.
- Verification: `ui-prototype/scripts/desktop-stages.cjs` drives the real app over WebView2 CDP, asserts the slider and play button are absent, walks the four pages and the 12 review labels, and writes `output/ui/desktop/stages-*.png`. The sample-preview suites (`capture.cjs`, `capture-dark.cjs`) now position the sample through a dev-only `window.__rtSeek` hook instead of the slider.
- Readability pass (same day): papers are collapsible cards (closed by default with a one-line summary; "Open all / Close all"; the final answer opens by itself), one column, full width with the side panel hidden while reading, and only the page scrolls (no nested scrollbars, enforced by the desktop script). Model text is rendered by a small safe Markdown renderer (`src/live/markdown.tsx`, React elements only), also used on the final answer page.
