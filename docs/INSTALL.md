# Roundtable 0.2: install and first use

No developer tools are needed. The installer is about 4 MB.

## Install (one minute)

1. Double-click `Roundtable_0.2.0_x64-setup.exe`.
2. Windows may show "Windows protected your PC" because the installer is not code-signed yet.
   Click **More info**, then **Run anyway**.
3. Click through the installer. It installs for your user only, needs no administrator password,
   and adds a desktop icon and a Start menu entry. If you had version 0.1, it is replaced; your runs are kept.

## First use (two minutes)

1. Open Roundtable from the desktop icon.
2. The Ask screen asks for your OpenRouter key. Paste it and click **Save key**.
   It is stored in Windows Credential Manager, never in a file.
3. A team of four agents appears: Gemini 3.8 Flash, GLM 5.3 Flash, DeepSeek V4 Pro and Qwen 3.8 Max,
   with Gemini as the final writer.
4. Type a question and press **Start**. The cost estimate is shown next to the button before anything is sent.

## The three places

- **Ask**: your question, your agents, and Start. While a run is going you see live cost, tokens,
  and which agent is being waited on.
- **History**: every past run. Click one to read the final answer and every step.
- **Settings**: API keys, default spending limit, dark or light theme.

## Agents

- **Add**: click the "+ Add agent" card, type a name, pick a model. Only models from providers with a saved key are listed.
- **Remove**: click the × on a card. Undo appears for a few seconds. At least two agents are needed.
- **Change what an agent is told**: click its card. The System prompt box has one tab per step
  (Answer, Review, Improve). What is written there is exactly what that model receives.
  "Reset to default" restores the original text.

## Good to know

- A four-agent run costs about $0.15 to $0.25 and takes 5 to 15 minutes. Thinking models such as GLM are slow;
  a single reply can take two minutes. The "Waiting for" timer shows it is working.
- If a run pauses, press **Resume**. Nothing that already succeeded is sent or charged again.
- The spending limit (default $1.00 per run) is never exceeded.
- Data lives in `%LOCALAPPDATA%\io.roundtable.desktop\data\roundtable.db`.
