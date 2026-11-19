# Sillune

[English](README.md)

Sillune est un assistant IA Windows. Le noyau, l’interface Leptos et le SDK de plugins sont écrits en Rust. Les packs locaux d’image, de voix et de 3D utilisent Vulkan.

## Démarrage

L’interface démarre en anglais. Dans **Settings → General → Appearance → Language**, choisis **Français**, puis enregistre.

1. Extraire l’archive Windows x64 et ouvrir **Sillune.exe**. L’application utilise le runtime WebView2 de Windows.
2. Dans **Réglages → Modèles**, choisir un fournisseur et renseigner sa clé. **API compatible ou locale** accepte un serveur compatible OpenAI, par exemple Ollama.
3. Dans **Réglages → Général**, choisir le dossier de travail et le profil d’autorisation.
4. Dans **Réglages → Modèles → Modèles locaux**, cliquer sur **Installer le pack** et choisir un dossier sur le disque souhaité. Sillune y crée un sous-dossier par pack et mémorise son emplacement. Le panneau affiche le chemin enregistré, la taille, l’origine, la licence et l’empreinte des fichiers. Les poids se téléchargent à la demande.

Le **Chat** recherche, lit et livre des fichiers à prévisualiser. Le bouton **Web** et les demandes explicites de recherche déclenchent une recherche avant la réponse du modèle. En cas d’échec du fournisseur IA, Sillune affiche les sources qu’il a récupérées. Pour une prévision à court terme avec une ville, Sillune interroge Open-Meteo. **Agent** ajoute les actions de fichiers, le terminal et les connecteurs. Une demande explicite de création d’un fichier vide sur le Bureau utilise directement l’outil de fichiers, avec le profil de permissions sélectionné.

Le sélecteur près de la saisie récupère le catalogue du fournisseur et garde le modèle choisi dans chaque conversation. **Effort** propose six crans de Minimal à Ultra. Sillune transmet l’effort compatible et l’affiche dans l’activité du tour.

**Image** propose SDXS, FLUX.2 klein et Z-Image Turbo. Les deux derniers peuvent générer à 1 024 × 1 024. **3D** génère un GLB texturé avec TRELLIS.2 4B Q8 via Vulkan. Joins un PNG ou un JPEG pour convertir directement l’image ou décris un objet pour générer d’abord son image de référence. Le pack 3D s’installe depuis **Réglages → Modèles locaux** et occupe environ 7,3 Go. Le texte vers 3D utilise aussi un modèle d’image installé. Shap-E reste accessible dans le sélecteur de modèles 3D.

**Bot** transcrit le micro ou un WAV avec LFM3. La voix de sortie se configure indépendamment du modèle de conversation. Le choix inclut LFM3 local et les voix Marin et Cedar si une clé OpenAI est enregistrée. Si aucun fournisseur distant n’est configuré, le pack LFM3 peut aussi rédiger la réponse. La dernière réponse vocale reste réécoutable en mémoire pendant l’ouverture de l’app. **Plugin** crée et installe une extension depuis une demande en français.

**Thèmes** crée et applique des palettes validées que l’on retrouve ensuite dans **Réglages → Général**. **Cobalt** est fourni, avec une palette bleu nuit et des accents électriques. Dans les mêmes réglages, on peut déplacer le sélecteur des modes dans la barre supérieure.

## Extensions et mémoire

Le dossier local `plugins/` contient un manifeste et un README par plugin. Le mode Plugin charge `plugins/SKILL.md`, qui explique le SDK et répertorie les extensions. Les minuteurs occupent la barre supérieure. Les onglets s’ouvrent depuis la barre latérale. Les widgets WebAssembly reçoivent les valeurs exposées par le SDK. Leur création demande Rust et la cible `wasm32-unknown-unknown` installés. Le panneau Plugins permet de consulter les README, d’activer, de désactiver et de désinstaller une extension ajoutée.

**Réglages → Mémoire** active la mémoire entre conversations. Le profil garde les notes modifiables, les suggestions à valider et un index local de l’historique. Les guides intégrés de Hermes Agent, Anthropic et Hallmark se lisent depuis **Réglages → Plugins**. Pour choisir un guide pour le tour, écrire par exemple `/skill systematic-debugging` ou `/skill hallmark`. En mode Agent, le modèle peut charger un guide et les ressources associées à la demande.

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

Pour les tests réels des moteurs, définir `SILLUNE_RUNTIME_TEST` sur un dossier Runtime contenant les packs SDXS, Z-Image, LFM3 et Shap-E installés. `SILLUNE_AUDIO_TEST` peut désigner un fichier PCM16 mono à 16 kHz pour tester la transcription. Ajouter `--test product -- --include-ignored --test-threads=1` pour les exécuter.

La qualification de bout en bout utilise un profil jetable et une clé OpenRouter lue sur l’entrée standard. Elle exerce les modes Chat, Agent, Plugin, Image et Bot. Tester également dans la fenêtre compilée l’ouverture des pièces jointes, le tableau de bord des extensions et la lecture de l’audio.

Le code de Sillune est sous licence MIT. Les instructions tierces sont distribuées avec leur licence et leur référence dans `skills/`.
