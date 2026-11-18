---
name: sillune-plugins
description: Créer et maintenir des plugins dans le profil Sillune.
---

# Guide de création de plugins Sillune

Crée le manifeste `plugin.toml` et un `README.md` par plugin. Un manifeste contient un identifiant en minuscules (ASCII et tirets), un titre, une description courte, une version et un type. Garde la description sur une ligne. Elle sert à produire la ligne du catalogue avec le chemin `plugins/<id>/`. Lis un README avec `plugin_docs` pour t’inspirer d’un plugin existant.

Types créables, un par plugin.

- `kind = "tab"` ajoute un onglet réel. `text` contient le texte affiché. Pour une mise en page riche, `html` s’affiche dans la vue isolée de l’onglet et peut inclure du CSS.
- `kind = "timer"` ajoute un minuteur interactif à droite de la barre supérieure. `seconds = 600` représente dix minutes. L’utilisateur peut lancer, mettre en pause et réinitialiser le décompte.
- `kind = "wasm"` ajoute un onglet calculé. Fournis dans `rust` un fichier Rust autonome sans dépendance. L’export `#[unsafe(no_mangle)] pub extern "C" fn sillune_tick(elapsed: i32, value: i32) -> i32` reçoit les secondes écoulées et la valeur choisie. Il renvoie un nombre affiché sous `text`. La VM fournit une limite de calcul et l’état de l’extension est isolé.

Exemple de manifeste pour le minuteur.

```toml
id = "minuteur"
title = "Minuteur"
description = "Compte à rebours de dix minutes en haut à droite"
kind = "timer"
version = "1.0.0"
seconds = 600
```

Soumets le manifeste, le README et le code Rust éventuel à `install_plugin`. L’outil valide le paquet, exerce son point d’intégration, montre les changements selon les permissions et le déplace dans le catalogue actif. Le panneau Plugins permet de l’essayer, le désactiver ou le retirer.

## Catalogue
