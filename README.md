# Sillune

Sillune est un assistant IA Windows. Le noyau, l’interface Leptos et le SDK de plugins sont écrits en Rust. Les packs d’image et de voix utilisent des moteurs Vulkan locaux.

## Démarrage

1. Extraire l’archive Windows x64 et ouvrir **Sillune.exe**. L’application utilise le runtime WebView2 de Windows.
2. Dans **Réglages → Modèles**, choisir un fournisseur et renseigner sa clé. **API compatible ou locale** accepte un serveur compatible OpenAI, par exemple Ollama.
3. Dans **Réglages → Général**, choisir le dossier de travail et le profil d’autorisation.
4. Pour l’image ou la voix hors ligne, installer les packs correspondants depuis **Réglages → Modèles**. Le panneau affiche la taille, l’origine, la licence et l’empreinte des fichiers.

Le **Chat** recherche, lit et livre des fichiers à prévisualiser. **Agent** ajoute les actions de fichiers, le terminal et les connecteurs. **Image** propose SDXS rapide et FLUX.2 klein en option, avec un choix de format et de graine. **Bot** capte le micro ou lit un WAV, transcrit avec LFM3, puis produit une réponse vocale française. Si aucun fournisseur distant n’est configuré, LFM3 traite aussi les réponses textuelles du Bot. **Plugin** crée et installe une extension depuis une demande en français.

## Extensions et mémoire

Le dossier local `plugins/` contient un manifeste et un README par plugin. Le mode Plugin charge `plugins/SKILL.md`, qui explique le SDK et répertorie les extensions. Les minuteurs occupent la barre supérieure. Les onglets s’ouvrent depuis la barre latérale. Les widgets WebAssembly reçoivent les valeurs exposées par le SDK. Leur création demande Rust et la cible `wasm32-unknown-unknown` installés. Le panneau Plugins permet de consulter les README, d’activer, de désactiver et de désinstaller une extension ajoutée.

**Réglages → Mémoire** active la mémoire entre conversations. Le profil garde les notes modifiables, les suggestions à valider et un index local de l’historique. Les skills de départ se lisent depuis **Réglages → Plugins**. Pour appliquer un skill à un tour, écrire `/skill frontend-design` ou `/skill test-driven-development` dans le message.

Pour Google Drive, enregistrer un jeton autorisé ou connecter dans le navigateur un client OAuth Google Desktop. Le périmètre `drive.file` regroupe les fichiers accordés à ce client et ceux que l’application crée. Le connecteur GitHub utilise un jeton limité aux dépôts souhaités. Les serveurs MCP locaux et HTTP se déclarent dans **Connecteurs**.

## Données et permissions

Sillune conserve l’historique, les pièces jointes, les prompts, la mémoire et les plugins personnels dans son profil Windows. Le Gestionnaire d’identifiants Windows garde les clés et les jetons. Les requêtes distantes transmettent le contexte utile au fournisseur sélectionné. L’audio du micro est transcrit localement et le texte transcrit sert de demande au modèle.

Le moteur vérifie les outils autorisés pour chaque mode. Les lectures et écritures de fichiers sont limitées au dossier choisi. Les commandes PowerShell et les outils MCP externes peuvent avoir des effets hors de ce dossier selon leurs droits. Le bouton d’annulation restaure les fichiers suivis avec vérification des retouches intervenues entre-temps.

## Compiler sous Windows

Prérequis local MSVC, Windows SDK, WebView2, Rust stable MSVC avec `wasm32-unknown-unknown` et [Trunk](https://trunkrs.dev/).

```powershell
rustup target add wasm32-unknown-unknown --toolchain stable-x86_64-pc-windows-msvc
.\scripts\Build.ps1
```

Le script compile l’interface avec Trunk, enchaîne sur Tauri et produit l’archive et son SHA-256 dans `out/`. Les packs installables sont référencés avec leurs empreintes dans `core/src/packs.json`.

## Vérifier

```powershell
cargo +stable-x86_64-pc-windows-msvc test -p sillune-core --locked
```

Pour les tests réels des moteurs, définir `SILLUNE_RUNTIME_TEST` sur un dossier Runtime contenant `sd/`, `llama/` et `Models/`, et éventuellement `SILLUNE_AUDIO_TEST` sur une source PCM16 mono à 16 kHz. Ajouter `--test product -- --include-ignored --test-threads=1` pour les exécuter.

La qualification de bout en bout utilise un profil jetable et une clé OpenRouter lue sur l’entrée standard. Elle exerce les modes Chat, Agent, Plugin, Image et Bot. Tester également dans la fenêtre compilée l’ouverture des pièces jointes, le tableau de bord des extensions et la lecture de l’audio.

Le code de Sillune est sous licence MIT. Les instructions tierces sont distribuées avec leur licence et leur référence dans `skills/`.
