---
name: sillune-theme
description: Créer et installer une palette accessible dans le mode Thèmes.
---

# Créer un thème Sillune

Appelle `install_theme` avec `manifest`, une chaîne contenant un objet JSON. Donne un identifiant court en minuscules avec des tirets, un nom affichable et `scheme` égal à `dark` ou `light`. `colors` contient des valeurs hexadécimales à six chiffres précédées de #. Fournis background, sidebar, surface, text, muted et accent. Tu peux ajouter border, accent_text, danger et success.

```json
{"id":"brume","name":"Brume","scheme":"dark","colors":{"background":"#101723","sidebar":"#131d2c","surface":"#1b2a40","text":"#f4f7ff","muted":"#b4c2db","accent":"#90b9ff","accent_text":"#111a2d","border":"#3a5173"}}
```

Teste au moins un contraste de 4,5:1 pour le texte courant et le texte sur l’accent. Évite les couleurs de fond trop proches pour la barre latérale et la zone principale. L’outil contrôle l’identifiant, les champs, les couleurs et les contrastes, puis enregistre et applique le thème selon les permissions. Il renvoie la confirmation de l’installation. Aucun fichier de style ou ressource externe n’est requis.
