# Roundtable demo — how to run it for a live review

## Start
Double-click **Roundtable** on the Desktop (also in this folder as `Roundtable.lnk`).
It opens a full-screen app window titled "Roundtable — Your thinking space". Nothing else appears.

## Stop
Close the window. The background server stops by itself.
If anything is left over, double-click **Stop Roundtable**.

## What it is
A design prototype with sample data. No AI keys, no live requests, no charges.
It draws its screens with the Windows web engine (Edge, in app mode with all browser UI removed).
The final product will be the same screens packaged as a real Windows .exe with Tauri.

## Suggested walkthrough (about 4 minutes)
1. **Welcome** — read the three promises. Click *Explore the sample workspace*.
2. **Home** — click *Plan a product launch* to fill the question. Point at the four seats. Click *Start roundtable*.
3. **Live Roundtable** — let the demo play. Click the *02 Peer reviews* stage, then click any seat card and open *Allowed context* to show what that model was and was not given. Press Esc.
4. Click *View sample answer*.
5. **Final Answer** — show the answer, the four perspectives on the right, and *The process at a glance*. Try *Copy* and *Export*.
6. **AI Panels** — show that every seat has its own model and three role prompts, and the final writer is separate.
7. **Settings** — toggle *Dark* / *Light* to show both themes. The sun/moon icon top-right does the same from any screen.

## Screen-share tips
- Share the **window** "Roundtable", not the whole screen.
- Turn on Windows Focus Assist / Do not disturb so notifications stay hidden.
- If a Microsoft sign-in or sync prompt ever appears the first time, close it once; it will not return.

## Rebuild after design changes
    cd ui-prototype
    npm run build
Then start the demo again.
