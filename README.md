# Sillune

[Lire en français](README.fr.md)

Sillune is an AI workspace for Windows. Its core, Leptos interface and plugin SDK are written in Rust. Local packs use Vulkan for image, voice and 3D generation. Create and tested with Intel Arc b580

## Get started

1. Download the [Windows x64 release](https://github.com/lol2007/sillune/releases/latest), extract it and open **Sillune.exe**. Sillune uses Microsoft Edge WebView2.
2. In **Settings → Models**, select a provider and enter your API key. **Compatible or local API** accepts OpenAI-compatible servers such as Ollama.
3. In **Settings → General**, choose a workspace and set the permission level. The interface starts in English. Select **Français** under **Appearance → Language** and save to switch.
4. Under **Settings → Models → Local models**, click **Install pack** and select a folder on the drive you want to use. Sillune remembers each pack's location. Model weights download on demand.

## Modes

| Mode | What you can do |
| --- | --- |
| **Chat** | Ask questions, search the web, read authorized files and preview attachments. Explicit web requests trigger a search before the model replies. |
| **Agent** | Work with files, run terminal commands and use connectors within the selected permissions. Changes tracked by Sillune can be undone. |
| **Image** | Generate images locally with SDXS, FLUX.2 klein or Z-Image Turbo. FLUX.2 and Z-Image can generate at 1024 × 1024. |
| **3D** | Generate textured GLBs from an image or a description with TRELLIS.2. Text-to-3D also uses an installed image model. Rotate the result in the app or export it. |
| **Bot** | Talk to Sillune and hear the reply. LFM3 transcribes locally and can read replies. OpenAI voices are also available with an API key. |
| **Plugin** | Ask Sillune to create a timer, sidebar tab or WebAssembly widget. |
| **Themes** | Create, apply and save custom palettes. Cobalt is included. |

The composer can retrieve the provider's model catalog and remember a model per conversation. The reasoning slider offers six levels from Minimal to Ultra and sends a level the model supports. Saved prompts and optional memory are available in the interface.

## Extensions and connectors

The local `plugins/` folder contains a manifest and README for each extension. Plugin mode loads `plugins/SKILL.md` as its SDK guide. Creating a WebAssembly widget requires Rust and the `wasm32-unknown-unknown` target. Manage extensions and browse built-in skills in **Settings → Plugins**.

Google Drive uses a token or a Google Desktop OAuth client with the `drive.file` scope. GitHub uses a token scoped to the repositories you want to access. Configure local or HTTP MCP servers in **Settings → Connectors**.

## Data and permissions

Sillune stores chats, attachments, saved prompts, notes and extensions in its Windows profile. API keys and tokens are stored in Windows Credential Manager. Remote requests send the relevant context to the selected provider. Microphone audio is transcribed locally and the transcript goes to the conversation model.

File tools are scoped to the workspace you choose. PowerShell commands and external MCP servers can act outside it according to their own permissions.

## Build and test

Install the Windows SDK, MSVC, WebView2, the stable MSVC Rust toolchain and [Trunk](https://trunkrs.dev/).

```powershell
rustup target add wasm32-unknown-unknown --toolchain stable-x86_64-pc-windows-msvc
.\scripts\Build.ps1
cargo +stable-x86_64-pc-windows-msvc test -p sillune-core --locked
```

The build writes the Windows archive and its SHA-256 checksum to `out/`. Model pack sources and checksums are tracked in `core/src/packs.json`. For local engine tests, set `SILLUNE_RUNTIME_TEST` to a folder containing the packs and run the product tests with `--include-ignored`.

Sillune is licensed under MIT. Third-party skills retain their own licenses and source references in `skills/`.
